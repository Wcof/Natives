// market-host: unified CN-market data engine (A-shares, on-exchange ETFs,
// OTC funds) serving NormalizedAssetItem snapshots/ticks over a loopback
// WebSocket. /goal 阶段二 deliverable.
package main

import (
	"context"
	"flag"
	"log"
	"os"
	"os/signal"
	"syscall"

	"natives/market-host/internal/engine"
	"natives/market-host/internal/ws"
)

func main() {
	port := flag.String("port", "8734", "loopback listen port")
	flag.Parse()

	hub := ws.NewHub()
	eng := engine.New(hub.Broadcast)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	// 采集循环：按订阅池与会话状态机轮询数据源（未启动则永远无 tick）
	go eng.Run(ctx)
	if err := ws.Serve(ctx, "127.0.0.1:"+*port, eng, hub); err != nil {
		log.Printf("serve: %v", err)
	}
	log.Printf("market-host exit")
}
