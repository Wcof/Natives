package main

import (
	"context"
	"io"
	"net"
	"os"
	"os/signal"
	"path/filepath"
	"slices"
	"syscall"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/host"
	"github.com/ldh/natives/model-host/internal/secrets"
	"github.com/ldh/natives/model-host/internal/singleinstance"
	log "github.com/sirupsen/logrus"
)

func main() {
	run(secrets.KeyringStore{})
}

func run(secretStore secrets.Store) {
	log.SetOutput(os.Stderr)
	log.SetLevel(log.WarnLevel)
	nativeOutput := os.Stdout
	os.Stdout = os.Stderr
	statePath, err := domain.DefaultStatePath()
	if err != nil {
		return
	}
	if !slices.Contains(os.Args[1:], workerArgument) {
		if err = bridgeModelHost(filepath.Dir(statePath), os.Stdin, nativeOutput); err != nil {
			log.Warn("model host connection failed")
		}
		return
	}
	prepareWorker()
	instance, relay, err := singleinstance.Acquire(filepath.Dir(statePath))
	if relay != nil {
		relayNativeMessages(relay, os.Stdin, nativeOutput)
		return
	}
	if err != nil {
		return
	}
	hub := newClientHub()
	engine, err := host.NewEngine(domain.NewRepository(statePath), secretStore, hub.broadcast)
	if err != nil {
		_ = instance.Close()
		return
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	_ = engine.Restore(ctx)
	accepted := make(chan net.Conn)
	go func() {
		defer close(accepted)
		for {
			connection, acceptErr := instance.Listener.Accept()
			if acceptErr != nil {
				return
			}
			// 退出联动：未交付的连接当场关闭，不悬挂在 channel 发送上（L4）。
			select {
			case accepted <- connection:
			case <-ctx.Done():
				_ = connection.Close()
				return
			}
		}
	}()
	hub.serve(ctx, engine, os.Stdin, nativeOutput, nativeOutput.Close)
loop:
	for {
		select {
		case connection, ok := <-accepted:
			if !ok {
				break loop
			}
			hub.serve(ctx, engine, connection, connection, connection.Close)
		case <-hub.idle:
			// 顶栏持有的 stdin 连接与浏览器页面同为合法客户端；
			// 只有全部客户端离开且非驻留时才退出（L1 owner 语义）。
			if hub.empty() && !engine.Resident() {
				break loop
			}
		case <-ctx.Done():
			break loop
		}
	}
	// L4 统一退出顺序：关闭自有客户端并解除阻塞 I/O → Engine 收口
	// （拒绝新请求 → 取消任务并等待 → 停用量 producer 并排空 → 关存储）
	// → 释放实例锁。
	hub.closeAll()
	engine.Close()
	_ = instance.Close()
}

func relayNativeMessages(connection net.Conn, input io.Reader, output io.Writer) {
	defer connection.Close()
	go func() {
		_, _ = io.Copy(connection, input)
		if stream, ok := connection.(interface{ CloseWrite() error }); ok {
			_ = stream.CloseWrite()
		} else {
			_ = connection.Close()
		}
	}()
	_, _ = io.Copy(output, connection)
}
