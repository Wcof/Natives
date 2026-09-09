//go:build windows

package singleinstance

import (
	"net"
	"time"

	"github.com/Microsoft/go-winio"
)

type Instance struct{ Listener net.Listener }

const pipe = `\\.\pipe\natives-model-host`

func Connect(string) (net.Conn, error) {
	return winio.DialPipe(pipe, ptr(150*time.Millisecond))
}

func Acquire(string) (*Instance, net.Conn, error) {
	if connection, err := Connect(""); err == nil {
		return nil, connection, nil
	}
	listener, err := winio.ListenPipe(pipe, &winio.PipeConfig{
		SecurityDescriptor: "D:P(A;;GA;;;OW)",
		MessageMode:        false,
		InputBufferSize:    1 << 20,
		OutputBufferSize:   1 << 20,
	})
	if err == nil {
		return &Instance{Listener: listener}, nil, nil
	}
	if connection, dialErr := winio.DialPipe(pipe, ptr(time.Second)); dialErr == nil {
		return nil, connection, nil
	}
	return nil, nil, err
}

func (i *Instance) Close() error {
	if i == nil || i.Listener == nil {
		return nil
	}
	return i.Listener.Close()
}

func ptr(value time.Duration) *time.Duration { return &value }
