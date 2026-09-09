package main

import (
	"context"
	"io"
	"net"
	"os"
	"os/signal"
	"path/filepath"
	"slices"
	"sync"
	"syscall"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/host"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
	"github.com/ldh/natives/model-host/internal/singleinstance"
	log "github.com/sirupsen/logrus"
)

type clientHub struct {
	mu      sync.Mutex
	writers map[*nativeio.Writer]bool
	idle    chan struct{}
}

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
	hub := &clientHub{writers: make(map[*nativeio.Writer]bool), idle: make(chan struct{}, 1)}
	engine, err := host.NewEngine(domain.NewRepository(statePath), secretStore, hub.broadcast)
	if err != nil {
		_ = instance.Close()
		return
	}
	defer func() {
		engine.Close()
		_ = instance.Close()
	}()
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	_ = engine.Restore(ctx)
	accepted := make(chan net.Conn)
	go func() {
		for {
			connection, acceptErr := instance.Listener.Accept()
			if acceptErr != nil {
				close(accepted)
				return
			}
			accepted <- connection
		}
	}()
	hub.serve(ctx, engine, os.Stdin, nativeOutput, nativeOutput.Close)
	for {
		select {
		case connection, ok := <-accepted:
			if !ok {
				return
			}
			hub.serve(ctx, engine, connection, connection, connection.Close)
		case <-hub.idle:
			if hub.empty() && !engine.Resident() {
				return
			}
		case <-ctx.Done():
			return
		}
	}
}

func (h *clientHub) serve(ctx context.Context, engine *host.Engine, input io.Reader, output io.Writer, closeClient func() error) {
	writer := nativeio.NewWriter(output)
	h.mu.Lock()
	h.writers[writer] = true
	h.mu.Unlock()
	go func() {
		defer func() {
			h.mu.Lock()
			delete(h.writers, writer)
			h.mu.Unlock()
			if closeClient != nil {
				_ = closeClient()
			}
			select {
			case h.idle <- struct{}{}:
			default:
			}
		}()
		for {
			request, err := nativeio.Read(input)
			if err != nil {
				return
			}
			if err = writer.Write(engine.Handle(ctx, request)); err != nil {
				return
			}
		}
	}()
}

func (h *clientHub) broadcast(response nativeio.Response) {
	h.mu.Lock()
	writers := make([]*nativeio.Writer, 0, len(h.writers))
	for writer := range h.writers {
		writers = append(writers, writer)
	}
	h.mu.Unlock()
	for _, writer := range writers {
		_ = writer.Write(response)
	}
}

func (h *clientHub) empty() bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return len(h.writers) == 0
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
