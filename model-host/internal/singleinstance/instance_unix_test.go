//go:build darwin || linux

package singleinstance

import (
	"net"
	"os"
	"testing"
	"time"
)

func TestAcquireReturnsOnePrimaryAndOneRelay(t *testing.T) {
	dir := t.TempDir()
	primary, relay, err := Acquire(dir)
	if err != nil || primary == nil || relay != nil {
		t.Fatalf("primary acquire = %#v %#v %v", primary, relay, err)
	}
	t.Cleanup(func() { _ = primary.Close() })
	accepted := make(chan net.Conn, 1)
	go func() { connection, _ := primary.Listener.Accept(); accepted <- connection }()
	second, relay, err := Acquire(dir)
	if err != nil || second != nil || relay == nil {
		t.Fatalf("relay acquire = %#v %#v %v", second, relay, err)
	}
	defer relay.Close()
	select {
	case connection := <-accepted:
		defer connection.Close()
	case <-time.After(time.Second):
		t.Fatal("primary did not accept relay")
	}
	info, err := os.Stat(primary.socketPath)
	if err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("socket permissions = %v %v", info, err)
	}
}
