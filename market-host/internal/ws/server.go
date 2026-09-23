// Package ws serves the single full-duplex loopback endpoint and routes
// client subscription frames to the engine.
package ws

import (
	"context"
	"encoding/json"
	"log"
	"net/http"
	"sync"
	"time"

	"natives/market-host/internal/engine"
	"natives/market-host/internal/model"
)

// clientFrame is the only front-end → host message shape (阶段三 §3).
type clientFrame struct {
	Action  string   `json:"action"` // subscribe | unsubscribe
	Symbols []string `json:"symbols"`
}

// Hub tracks connected clients and broadcasts engine batches.
type Hub struct {
	mu      sync.RWMutex
	clients map[*clientConn]struct{}
}

type clientConn struct {
	send chan []byte
}

// NewHub creates an empty hub.
func NewHub() *Hub { return &Hub{clients: map[*clientConn]struct{}{}} }

// Broadcast fans a merged batch out to every connected client (non-blocking:
// a slow client's channel fills and it is dropped rather than stalling ticks).
func (h *Hub) Broadcast(items []model.NormalizedAssetItem) {
	if len(items) == 0 {
		return
	}
	data, err := json.Marshal(model.Envelope{Type: "tick", Items: items})
	if err != nil {
		return
	}
	h.mu.RLock()
	defer h.mu.RUnlock()
	for c := range h.clients {
		select {
		case c.send <- data:
		default: // slow consumer: drop this batch for it
		}
	}
}

// Handler returns the upgrade endpoint (loopback only — the server itself
// must bind 127.0.0.1; this handler additionally rejects non-loopback remotes).
func (h *Hub) Handler(e *engine.Engine) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if ip := r.RemoteAddr; !isLoopback(ip) {
			http.Error(w, "loopback only", http.StatusForbidden)
			return
		}
		conn, err := upgrade(w, r)
		if err != nil {
			log.Printf("upgrade: %v", err)
			return
		}
		c := &clientConn{send: make(chan []byte, 64)}
		h.mu.Lock()
		h.clients[c] = struct{}{}
		h.mu.Unlock()
		defer func() {
			h.mu.Lock()
			delete(h.clients, c)
			h.mu.Unlock()
			conn.Close()
		}()

		// writer pump
		done := make(chan struct{})
		go func() {
			defer close(done)
			for data := range c.send {
				if err := conn.Write(data); err != nil {
					return
				}
			}
		}()

		// reader pump
		for {
			raw, err := conn.Read()
			if err != nil {
				break
			}
			var frame clientFrame
			if err := json.Unmarshal(raw, &frame); err != nil {
				continue
			}
			switch frame.Action {
			case "subscribe":
				snap := e.Subscribe(frame.Symbols)
				if data, err := json.Marshal(model.Envelope{Type: "snapshot", Items: snap}); err == nil {
					select {
					case c.send <- data:
					default:
					}
				}
			case "unsubscribe":
				e.Unsubscribe(frame.Symbols)
			}
		}
		// panel closed / socket dropped: pause the stream for its symbols
		// unless another client still subscribes them.
		e.Unsubscribe(nil) // 阶段三 §3: 关闭面板自动暂停推流
		_ = done
	}
}

func isLoopback(remote string) bool {
	for _, p := range []string{"127.0.0.1", "[::1]", "localhost"} {
		if len(remote) >= len(p) && remote[:len(p)] == p {
			return true
		}
	}
	return false
}

// Serve binds loopback and blocks until ctx is cancelled.
func Serve(ctx context.Context, addr string, e *engine.Engine, hub *Hub) error {
	mux := http.NewServeMux()
	mux.HandleFunc("/ws", hub.Handler(e))
	srv := &http.Server{Addr: addr, Handler: mux, ReadHeaderTimeout: 5 * time.Second}
	go func() {
		<-ctx.Done()
		shutdown, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutdown)
	}()
	log.Printf("market-host listening on ws://%s/ws", addr)
	return srv.ListenAndServe()
}
