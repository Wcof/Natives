//go:build darwin || linux

package main

import (
	"os/exec"
	"os/signal"
	"syscall"
)

func configureWorker(command *exec.Cmd) {
	command.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
}

func prepareWorker() {
	// A browser disconnect can race a final response on the bridge pipe.
	signal.Ignore(syscall.SIGPIPE)
}
