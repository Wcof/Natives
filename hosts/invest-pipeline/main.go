package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool {
		// 允许来自 Chrome 扩展与本地沙箱的连接
		return true
	},
}

type Client struct {
	conn        *websocket.Conn
	send        chan []byte
	symbols     map[string]bool
	mu          sync.RWMutex
}

type Hub struct {
	clients    map[*Client]bool
	register   chan *Client
	unregister chan *Client
	broadcast  chan []byte
	mu         sync.RWMutex
}

var hub = Hub{
	clients:    make(map[*Client]bool),
	register:   make(chan *Client),
	unregister: make(chan *Client),
	broadcast:  make(chan []byte),
}

func (h *Hub) run() {
	for {
		select {
		case client := <-h.register:
			h.mu.Lock()
			h.clients[client] = true
			h.mu.Unlock()
			log.Printf("[WS] Client connected. Total: %d", len(h.clients))

		case client := <-h.unregister:
			h.mu.Lock()
			if _, ok := h.clients[client]; ok {
				delete(h.clients, client)
				close(client.send)
			}
			h.mu.Unlock()
			log.Printf("[WS] Client disconnected. Total: %d", len(h.clients))

		case message := <-h.broadcast:
			h.mu.RLock()
			for client := range h.clients {
				select {
				case client.send <- message:
				default:
					close(client.send)
					delete(h.clients, client)
				}
			}
			h.mu.RUnlock()
		}
	}
}

// GetAllSubscribedSymbols 获取当前所有客户端订阅的唯一标的集合
func (h *Hub) GetAllSubscribedSymbols() []string {
	h.mu.RLock()
	defer h.mu.RUnlock()

	unique := make(map[string]bool)
	for client := range h.clients {
		client.mu.RLock()
		for s := range client.symbols {
			unique[s] = true
		}
		client.mu.RUnlock()
	}

	res := make([]string, 0, len(unique))
	for s := range unique {
		res = append(res, s)
	}
	return res
}

func handleWebSocket(w http.ResponseWriter, r *http.Request) {
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("[WS Upgrade error]: %v", err)
		return
	}

	client := &Client{
		conn:    conn,
		send:    make(chan []byte, 256),
		symbols: make(map[string]bool),
	}

	hub.register <- client

	// 启动写协程
	go func() {
		defer conn.Close()
		for message := range client.send {
			if err := conn.WriteMessage(websocket.TextMessage, message); err != nil {
				return
			}
		}
	}()

	// 读协程：处理前端订阅消息
	defer func() {
		hub.unregister <- client
		conn.Close()
	}()

	// 默认自动订阅核心基准标的
	client.mu.Lock()
	client.symbols["sh600519"] = true
	client.symbols["sh000001"] = true
	client.symbols["005827"] = true
	client.mu.Unlock()

	for {
		_, message, err := conn.ReadMessage()
		if err != nil {
			break
		}

		var req ClientMessage
		if err := json.Unmarshal(message, &req); err == nil {
			client.mu.Lock()
			switch req.Action {
			case "subscribe":
				for _, s := range req.Symbols {
					client.symbols[strings.TrimSpace(s)] = true
				}
				log.Printf("[WS] Subscribed symbols: %v", req.Symbols)
			case "unsubscribe":
				for _, s := range req.Symbols {
					delete(client.symbols, strings.TrimSpace(s))
				}
			}
			client.mu.Unlock()
		}
	}
}

// 调度引擎：按需定时采集并广播
func startPollingEngine() {
	lastSparklineCache := make(map[string][]float64)

	for {
		status, interval := GetCurrentMarketStatus()

		symbols := hub.GetAllSubscribedSymbols()
		if len(symbols) > 0 {
			// 分离股票/ETF 与 场外公募
			stockSymbols := make([]string, 0)
			fundSymbols := make([]string, 0)

			for _, s := range symbols {
				clean := strings.TrimSpace(s)
				if strings.HasPrefix(clean, "sh") || strings.HasPrefix(clean, "sz") || strings.HasPrefix(clean, "bj") {
					stockSymbols = append(stockSymbols, clean)
				} else if len(clean) == 6 && (strings.HasPrefix(clean, "6") || strings.HasPrefix(clean, "5") || strings.HasPrefix(clean, "0") || strings.HasPrefix(clean, "3") || strings.HasPrefix(clean, "1")) {
					// 6位代码：如果是005827等场外，或者股票
					// 约定如果纯数字且首位是0且非000001/000858等常用股票，或者在基金列表中的视作场外
					if clean == "005827" || clean == "161725" || clean == "110011" {
						fundSymbols = append(fundSymbols, clean)
					} else {
						stockSymbols = append(stockSymbols, clean)
					}
				} else {
					fundSymbols = append(fundSymbols, clean)
				}
			}

			// 1. 采集股票行情
			items := make([]*NormalizedAssetItem, 0)
			if len(stockSymbols) > 0 {
				if stockMap, err := FetchStockQuotes(stockSymbols); err == nil {
					for _, it := range stockMap {
						// 挂载或更新 sparkline
						if len(it.Sparkline) <= 2 {
							if cached, ok := lastSparklineCache[it.Symbol]; ok && len(cached) > 2 {
								it.Sparkline = cached
							} else {
								sp := FetchStockSparkline(it.Symbol)
								if len(sp) > 0 {
									it.Sparkline = sp
									lastSparklineCache[it.Symbol] = sp
								}
							}
						}
						items = append(items, it)
					}
				}
			}

			// 2. 采集场外基金
			for _, fcode := range fundSymbols {
				if fitem, err := FetchFundQuote(fcode); err == nil {
					items = append(items, fitem)
				}
			}

			// 3. 广播快照与状态
			if len(items) > 0 {
				msg := ServerMessage{
					Type:         "snapshot",
					Data:         items,
					MarketStatus: string(status),
					Timestamp:    time.Now().UnixMilli(),
				}
				if payload, err := json.Marshal(msg); err == nil {
					hub.broadcast <- payload
				}
			}
		}

		time.Sleep(interval)
	}
}

func main() {
	port := flag.Int("port", 8765, "WebSocket service port")
	flag.Parse()

	go hub.run()
	go startPollingEngine()

	http.HandleFunc("/ws", handleWebSocket)
	http.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		status, _ := GetCurrentMarketStatus()
		fmt.Fprintf(w, `{"ok":true,"marketStatus":"%s","time":"%s"}`, status, time.Now().Format(time.RFC3339))
	})

	addr := fmt.Sprintf("127.0.0.1:%d", *port)
	log.Printf("==================================================")
	log.Printf("🚀 Natives Invest Pipeline Host started at ws://%s/ws", addr)
	log.Printf("   Market State Machine Active (Auto Trading/Closed frequency)")
	log.Printf("==================================================")

	if err := http.ListenAndServe(addr, nil); err != nil {
		log.Fatalf("Server failed: %v", err)
	}
}
