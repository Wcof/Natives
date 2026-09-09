//go:build windows

package main

import (
	"os/exec"
	"syscall"
)

func configureWorker(command *exec.Cmd) {
	const detachedProcess = 0x00000008
	const createNewProcessGroup = 0x00000200
	const createBreakawayFromJob = 0x01000000
	command.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: detachedProcess | createNewProcessGroup | createBreakawayFromJob,
		HideWindow:    true,
	}
}

func prepareWorker() {}
