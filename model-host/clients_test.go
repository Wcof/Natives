package main

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"io"
	"net"
	"path/filepath"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/host"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func newTestEngine(t *testing.T) *host.Engine {
	t.Helper()
	engine, err := host.NewEngine(domain.NewRepository(filepath.Join(t.TempDir(), "state.json")), secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	return engine
}

func writeRequestTolerant(writer io.Writer, request nativeio.Request) {
	payload, _ := json.Marshal(request)
	var head [4]byte
	binary.LittleEndian.PutUint32(head[:], uint32(len(payload)))
	_, _ = writer.Write(head[:])
	_, _ = writer.Write(payload)
}

// readResponseOptional 非致命读取：对端在 deadline 内未输出时返回 false。
func readResponseOptional(reader io.Reader) (nativeio.Response, bool) {
	var size uint32
	if err := binary.Read(reader, binary.LittleEndian, &size); err != nil {
		return nativeio.Response{}, false
	}
	payload := make([]byte, size)
	if _, err := io.ReadFull(reader, payload); err != nil {
		return nativeio.Response{}, false
	}
	var response nativeio.Response
	if json.Unmarshal(payload, &response) != nil {
		return nativeio.Response{}, false
	}
	return response, true
}

func readResponseT(t *testing.T, reader io.Reader) nativeio.Response {
	t.Helper()
	response, ok := readResponseOptional(reader)
	if !ok {
		t.Fatal("expected a response frame")
	}
	return response
}

func waitForIdle(t *testing.T, hub *clientHub, timeout time.Duration) {
	t.Helper()
	select {
	case <-hub.idle:
	case <-time.After(timeout):
		t.Fatal("client removal was not observed")
	}
}

// TestClientAdmissionLimit 验收 L04：连接达到 32 上限时第 33 个被拒绝，
// 收到结构化错误后关闭；已有客户端不受影响。
func TestClientAdmissionLimit(t *testing.T) {
	engine := newTestEngine(t)
	hub := newClientHub()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	type pipePair struct {
		client net.Conn
		host   net.Conn
	}
	clients := make([]pipePair, 0, maxClients)
	t.Cleanup(func() {
		for _, pair := range clients {
			_ = pair.client.Close()
		}
	})
	for i := 0; i < maxClients; i++ {
		client, hostEnd := net.Pipe()
		hub.serve(ctx, engine, hostEnd, hostEnd, hostEnd.Close)
		clients = append(clients, pipePair{client: client, host: hostEnd})
	}

	// 第 33 个连接：拒绝并返回结构化错误帧。
	rejected, rejectHost := net.Pipe()
	defer rejected.Close()
	rejectedDone := make(chan struct{})
	go func() {
		hub.serve(ctx, engine, rejectHost, rejectHost, rejectHost.Close)
		close(rejectedDone)
	}()
	_ = rejected.SetReadDeadline(time.Now().Add(2 * time.Second))
	response := readResponseT(t, rejected)
	if response.OK || response.ErrorCode != "host_busy" {
		t.Fatalf("expected host_busy rejection, got %+v", response)
	}
	select {
	case <-rejectedDone:
	case <-time.After(2 * time.Second):
		t.Fatal("rejected serve did not close promptly")
	}

	// 已在册客户端仍然可用。
	writeRequest(t, clients[0].client, nativeio.Request{ID: "ping", Method: "model_collect_status", Params: json.RawMessage(`{}`)})
	_ = clients[0].client.SetReadDeadline(time.Now().Add(2 * time.Second))
	if response := readResponseT(t, clients[0].client); !response.OK {
		t.Fatalf("existing client broken by admission limit: %+v", response)
	}
}

// TestSlowClientRemovedWithoutBlockingOthers 验收 L04：一个不读输出的客户端
// 在响应入队超限后被移除，期间不阻塞广播与其他客户端。
func TestSlowClientRemovedWithoutBlockingOthers(t *testing.T) {
	engine := newTestEngine(t)
	hub := newClientHub()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	slowClient, slowHost := net.Pipe()
	defer slowClient.Close()
	hub.serve(ctx, engine, slowHost, slowHost, slowHost.Close)

	goodClient, goodHost := net.Pipe()
	defer goodClient.Close()
	hub.serve(ctx, engine, goodHost, goodHost, goodHost.Close)

	// 慢客户端连发请求但不读响应：响应队列满 → 250ms 超限 → 移除（idle 信号）。
	go func() {
		for i := 0; i < clientResponseSlots+16; i++ {
			writeRequestTolerant(slowClient, nativeio.Request{ID: "slow", Method: "model_collect_status", Params: json.RawMessage(`{}`)})
		}
	}()
	// 第一次 idle = 慢客户端被移除；goodClient 仍在册。
	waitForIdle(t, hub, 5*time.Second)
	if hub.empty() {
		t.Fatal("healthy client must stay registered")
	}

	// 广播不阻塞：合并事件可达健康客户端。
	broadcastDeadline := time.Now().Add(2 * time.Second)
	for i := 0; time.Now().Before(broadcastDeadline) && i < 200; i++ {
		hub.broadcast(nativeio.Response{OK: true, Event: "model_usage_updated"})
	}
	_ = goodClient.SetReadDeadline(time.Now().Add(2 * time.Second))
	response := readResponseT(t, goodClient)
	if response.Event != "model_usage_updated" {
		t.Fatalf("expected merged event on healthy client, got %+v", response)
	}
}

// TestBroadcastMergesSameEventPerClient 验收 L04：同型状态事件按类型合并，
// 健康客户端最终只看到最新一条。
func TestBroadcastMergesSameEventPerClient(t *testing.T) {
	engine := newTestEngine(t)
	hub := newClientHub()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	client, hostEnd := net.Pipe()
	defer client.Close()
	hub.serve(ctx, engine, hostEnd, hostEnd, hostEnd.Close)

	hub.broadcast(nativeio.Response{OK: true, Event: "model_gateway_state_changed", Result: "first"})
	hub.broadcast(nativeio.Response{OK: true, Event: "model_gateway_state_changed", Result: "second"})
	hub.broadcast(nativeio.Response{OK: true, Event: "model_gateway_state_changed", Result: "third"})

	_ = client.SetReadDeadline(time.Now().Add(2 * time.Second))
	first := readResponseT(t, client)
	payload, _ := json.Marshal(first.Result)
	if string(payload) != `"third"` {
		t.Fatalf("expected merged latest event, got %s", payload)
	}
	// 合并后没有残留旧事件：deadline 内不应再有同型事件到达。
	_ = client.SetReadDeadline(time.Now().Add(200 * time.Millisecond))
	if extra, ok := readResponseOptional(client); ok && extra.Event != "" {
		t.Fatalf("unexpected extra event after merge: %+v", extra)
	}
}

// TestCloseAllUnblocksReaderAndPump 验收 L05：shutdown 时阻塞中的读 goroutine
// 与写泵被解除阻塞，closeAll 在预算内返回，重复调用幂等。
func TestCloseAllUnblocksReaderAndPump(t *testing.T) {
	engine := newTestEngine(t)
	hub := newClientHub()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	client, hostEnd := net.Pipe()
	defer client.Close()
	hub.serve(ctx, engine, hostEnd, hostEnd, hostEnd.Close) // 不发请求：readLoop 阻塞在 Read

	done := make(chan struct{})
	go func() {
		hub.closeAll()
		hub.closeAll() // 幂等
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("closeAll did not return within budget")
	}
}

func TestClientCloseCancelsOnlyItsContext(t *testing.T) {
	firstContext, firstCancel := context.WithCancel(context.Background())
	defer firstCancel()
	secondContext, secondCancel := context.WithCancel(context.Background())
	defer secondCancel()
	hub := newClientHub()
	first := &client{id: 1, closing: make(chan struct{}), cancel: firstCancel}
	second := &client{id: 2, closing: make(chan struct{}), cancel: secondCancel}
	hub.clients[1], hub.clients[2] = first, second
	hub.remove(first, false)
	if firstContext.Err() == nil {
		t.Fatal("closed client's task context was not cancelled")
	}
	if secondContext.Err() != nil {
		t.Fatal("closing one client cancelled another client")
	}
	hub.remove(first, false)
	hub.remove(second, false)
}
