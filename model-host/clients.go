package main

// clientHub：Native Messaging 客户端连接的登记、有界输出与一致回收（方案 L1/L3）。
//
// 每 client 一条读 goroutine（读帧 → Handle → 入队响应）与一条写泵 goroutine
// （队列 → 帧输出）。边界（L3 初始内部限制，实测见 host-lifecycle 证据）：
//   - 客户端 admission 硬上限 32；第 33 个连接收到结构化错误后关闭；
//   - 每 client 响应队列 64 槽、输出（响应+事件）总字节 ≤ 2 MiB；
//   - 响应入队最长等待 250 ms，超限关闭该 client——请求响应不静默丢弃，
//     但持续不读的慢客户端会被移除，不阻塞其他客户端与用量持久化；
//   - 状态事件按事件类型合并（同型新事件替换未发送的旧事件），入队不阻塞。
//
// 清理顺序（L4）：从 hub 移除（后续广播不再到达）→ 关闭连接（解除阻塞 I/O）
// → 取消该 client 发起的推理任务。业务锁不做任何等待。

import (
	"bytes"
	"context"
	"encoding/binary"
	"io"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/ldh/natives/model-host/internal/host"
	"github.com/ldh/natives/model-host/internal/nativeio"
)

const (
	maxClients           = 32
	clientResponseSlots  = 64
	clientOutputMaxBytes = 2 << 20 // 2 MiB
	clientEnqueueWait    = 250 * time.Millisecond
	clientEOFFlushWait   = 250 * time.Millisecond
)

type clientHub struct {
	mu       sync.Mutex
	clients  map[int64]*client
	nextID   int64
	idle     chan struct{}
	wg       sync.WaitGroup
	stopping bool
}

type client struct {
	id        int64
	engine    *host.Engine
	writer    *nativeio.Writer
	closer    func() error
	responses chan []byte
	evMu      sync.Mutex
	events    map[string][]byte
	evReady   chan struct{}
	pending   atomic.Int64
	closing   chan struct{}
	closeOnce sync.Once
	owners    sync.Map
	cancel    context.CancelFunc
}

func newClientHub() *clientHub {
	return &clientHub{
		clients: make(map[int64]*client),
		idle:    make(chan struct{}, 1),
	}
}

func (h *clientHub) empty() bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return len(h.clients) == 0
}

// serve 登记一个新客户端；admission 满或已进入 stopping 时返回结构化错误并关闭。
func (h *clientHub) serve(ctx context.Context, engine *host.Engine, input io.Reader, output io.Writer, closeClient func() error) {
	h.mu.Lock()
	if h.stopping {
		h.mu.Unlock()
		rejectClient(output, "host_stopping", "模型服务正在退出")
		_ = closeClient()
		return
	}
	if len(h.clients) >= maxClients {
		h.mu.Unlock()
		rejectClient(output, "host_busy", "客户端连接数已达上限")
		_ = closeClient()
		return
	}
	clientContext, cancel := context.WithCancel(ctx)
	c := &client{
		cancel:    cancel,
		id:        h.nextID,
		engine:    engine,
		writer:    nativeio.NewWriter(output),
		closer:    closeClient,
		responses: make(chan []byte, clientResponseSlots),
		events:    make(map[string][]byte),
		evReady:   make(chan struct{}, 1),
		closing:   make(chan struct{}),
	}
	h.nextID++
	h.clients[c.id] = c
	h.wg.Add(2)
	h.mu.Unlock()
	// 新客户端连入：异步触发一次增量日志采集（内置 10s 防抖，无定时器）
	engine.TriggerCollectAsync()
	go h.readLoop(clientContext, c, input)
	go h.outputPump(c)
}

// rejectClient 在正式登记前返回结构化错误帧（带写期限，避免被慢对端拖住）。
func rejectClient(output io.Writer, code, message string) {
	payload, err := nativeio.MarshalResponse(nativeio.Response{OK: false, Error: message, ErrorCode: code})
	if err != nil {
		return
	}
	if conn, ok := output.(net.Conn); ok {
		_ = conn.SetWriteDeadline(time.Now().Add(clientEnqueueWait))
	}
	var frame bytes.Buffer
	var prefix [4]byte
	binary.LittleEndian.PutUint32(prefix[:], uint32(len(payload)))
	frame.Write(prefix[:])
	frame.Write(payload)
	_, _ = output.Write(frame.Bytes())
}

// readLoop 单一帧读取者（R-B8）：读请求 → Handle → 有界入队响应。
func (h *clientHub) readLoop(ctx context.Context, c *client, input io.Reader) {
	defer h.wg.Done()
	defer h.remove(c, true)
	for {
		request, err := nativeio.Read(input)
		if err != nil {
			return
		}
		if request.Method == "model_inference_start" {
			if owner := host.InferenceOwner(request.Params); owner != "" {
				c.owners.Store(owner, struct{}{})
			}
		}
		response := c.engine.Handle(ctx, request)
		payload, err := nativeio.MarshalResponse(response)
		if err != nil {
			return
		}
		if !h.enqueueResponse(c, payload) {
			return
		}
	}
}

// outputPump 消费响应队列与合并事件，写出到客户端；写失败或关闭即退出。
func (h *clientHub) outputPump(c *client) {
	defer h.wg.Done()
	for {
		select {
		case payload := <-c.responses:
			c.pending.Add(-int64(len(payload) + 4))
			if err := c.writer.WriteEncoded(payload); err != nil {
				h.remove(c, false)
				return
			}
		case <-c.evReady:
			for {
				c.evMu.Lock()
				var evType string
				var payload []byte
				for name, data := range c.events {
					evType, payload = name, data
					break
				}
				if payload == nil {
					c.evMu.Unlock()
					break
				}
				delete(c.events, evType)
				c.evMu.Unlock()
				c.pending.Add(-int64(len(payload) + 4))
				if err := c.writer.WriteEncoded(payload); err != nil {
					h.remove(c, false)
					return
				}
			}
		case <-c.closing:
			return
		}
	}
}

// enqueueResponse 有界入队请求响应：队列长期满（250ms）时关闭该慢客户端，
// 不让它拖住读 goroutine、其他客户端或用量持久化。
func (h *clientHub) enqueueResponse(c *client, payload []byte) bool {
	size := int64(len(payload) + 4)
	if c.pending.Add(size) > clientOutputMaxBytes {
		c.pending.Add(-size)
		h.remove(c, false)
		return false
	}
	timer := time.NewTimer(clientEnqueueWait)
	defer timer.Stop()
	select {
	case c.responses <- payload:
		return true
	case <-timer.C:
		h.remove(c, false)
		return false
	case <-c.closing:
		c.pending.Add(-size)
		return false
	}
}

// enqueueEvent 按事件类型合并状态通知：同型未发送事件被替换（合并语义），
// 入队永不阻塞；输出字节同受 2 MiB 上限约束。
func (h *clientHub) enqueueEvent(c *client, eventType string, payload []byte) {
	size := int64(len(payload) + 4)
	c.evMu.Lock()
	if old, ok := c.events[eventType]; ok {
		c.pending.Add(-int64(len(old) + 4))
		delete(c.events, eventType)
	}
	if c.pending.Add(size) > clientOutputMaxBytes {
		c.pending.Add(-size)
		c.evMu.Unlock()
		return
	}
	c.events[eventType] = payload
	c.evMu.Unlock()
	select {
	case c.evReady <- struct{}{}:
	default:
	}
}

// broadcast 向全部客户端合并投递事件通知；不阻塞、不占用请求路径
// （用量持久化不依赖广播，L3）。
func (h *clientHub) broadcast(response nativeio.Response) {
	if response.Event == "" {
		return
	}
	payload, err := nativeio.MarshalResponse(response)
	if err != nil {
		return
	}
	h.mu.Lock()
	clients := make([]*client, 0, len(h.clients))
	for _, c := range h.clients {
		clients = append(clients, c)
	}
	h.mu.Unlock()
	for _, c := range clients {
		h.enqueueEvent(c, response.Event, payload)
	}
}

// remove 按一致顺序移除客户端：移出 hub（广播不再到达）→ 关闭连接
// （解除阻塞的读与写）→ 取消该 client 发起的推理任务 → 发空闲信号。
// flushOnEOF 用于正常读 EOF：给最后一批响应 250ms 冲刷窗口。
func (h *clientHub) remove(c *client, flushOnEOF bool) {
	h.mu.Lock()
	_, exists := h.clients[c.id]
	if exists {
		delete(h.clients, c.id)
	}
	h.mu.Unlock()
	if !exists {
		return
	}
	if flushOnEOF {
		deadline := time.Now().Add(clientEOFFlushWait)
		for time.Now().Before(deadline) {
			if c.pending.Load() == 0 && len(c.responses) == 0 {
				break
			}
			time.Sleep(5 * time.Millisecond)
		}
	}
	c.closeOnce.Do(func() {
		close(c.closing)
		if c.cancel != nil {
			c.cancel()
		}
	})
	if c.closer != nil {
		_ = c.closer()
	}
	if c.engine != nil {
		owners := make([]string, 0)
		c.owners.Range(func(key, _ any) bool {
			if name, ok := key.(string); ok {
				owners = append(owners, name)
			}
			return true
		})
		c.engine.CancelInferenceForOwners(owners)
	}
	select {
	case h.idle <- struct{}{}:
	default:
	}
}

// closeAll 进入停止状态：拒绝新客户端，关闭全部连接并等待读写 goroutine 退出
// （写泵被 closer 解除阻塞；读 goroutine 对 socket 由连接关闭解除，对 stdin
// 由进程退出回收——stdin 无法在 POSIX 下可靠解除阻塞，但 Engine 关闭后
// Handle 拒绝新请求，不会触碰已关闭存储）。
func (h *clientHub) closeAll() {
	h.mu.Lock()
	h.stopping = true
	clients := make([]*client, 0, len(h.clients))
	for _, c := range h.clients {
		clients = append(clients, c)
	}
	h.mu.Unlock()
	for _, c := range clients {
		c.closeOnce.Do(func() { close(c.closing) })
		if c.closer != nil {
			_ = c.closer()
		}
	}
	done := make(chan struct{})
	go func() {
		h.wg.Wait()
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(500 * time.Millisecond):
		// 残留 goroutine 随进程退出回收；Engine.closed 已拒绝其后续存储访问。
	}
}
