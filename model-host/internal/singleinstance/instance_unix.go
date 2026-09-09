//go:build darwin || linux

package singleinstance

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"syscall"
	"time"
)

type Instance struct {
	Listener   net.Listener
	lock       *os.File
	socketPath string
}

func controlSocketPath(dir string) string {
	path := filepath.Join(dir, "control.sock")
	if len(path) > 90 {
		digest := sha256.Sum256([]byte(dir))
		path = filepath.Join(os.TempDir(), fmt.Sprintf("natives-model-%d-%x.sock", os.Getuid(), digest[:8]))
	}
	return path
}

func Connect(dir string) (net.Conn, error) {
	return net.DialTimeout("unix", controlSocketPath(dir), 150*time.Millisecond)
}

func Acquire(dir string) (*Instance, net.Conn, error) {
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, nil, err
	}
	socketPath := controlSocketPath(dir)
	if connection, err := Connect(dir); err == nil {
		return nil, connection, nil
	}
	lock, err := os.OpenFile(filepath.Join(dir, "instance.lock"), os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, nil, err
	}
	if err = syscall.Flock(int(lock.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		_ = lock.Close()
		for range 20 {
			if connection, dialErr := net.DialTimeout("unix", socketPath, 100*time.Millisecond); dialErr == nil {
				return nil, connection, nil
			}
			time.Sleep(50 * time.Millisecond)
		}
		return nil, nil, errors.New("model host instance is starting")
	}
	_ = os.Remove(socketPath)
	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		_ = syscall.Flock(int(lock.Fd()), syscall.LOCK_UN)
		_ = lock.Close()
		return nil, nil, err
	}
	if err = os.Chmod(socketPath, 0o600); err != nil {
		_ = listener.Close()
		_ = os.Remove(socketPath)
		_ = syscall.Flock(int(lock.Fd()), syscall.LOCK_UN)
		_ = lock.Close()
		return nil, nil, err
	}
	return &Instance{Listener: listener, lock: lock, socketPath: socketPath}, nil, nil
}

func (i *Instance) Close() error {
	if i == nil {
		return nil
	}
	err := i.Listener.Close()
	_ = os.Remove(i.socketPath)
	_ = syscall.Flock(int(i.lock.Fd()), syscall.LOCK_UN)
	_ = i.lock.Close()
	return err
}
