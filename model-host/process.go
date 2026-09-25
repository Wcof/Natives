package main

import (
	"io"
	"os"
	"os/exec"
	"time"

	"github.com/ldh/natives/model-host/internal/singleinstance"
)

const workerArgument = "--model-host-worker"

// workerRestartExitCode: 内核在线升级换装后，worker 以该退出码请求桥接进程
// 原位重建自己，从而加载新二进制；Chrome 的 Native Messaging 连接保持不断。
const workerRestartExitCode = 75

func bridgeModelHost(dir string, input io.Reader, output io.Writer) error {
	if connection, err := singleinstance.Connect(dir); err == nil {
		relayNativeMessages(connection, input, output)
		return nil
	}
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	for attempt := 0; attempt < 3; attempt++ {
		code, spawnErr := spawnWorkerAndRelay(executable, input, output)
		if spawnErr != nil {
			return spawnErr
		}
		if code != workerRestartExitCode {
			return nil
		}
	}
	return nil
}

func spawnWorkerAndRelay(executable string, input io.Reader, output io.Writer) (int, error) {
	workerInput, bridgeInput, err := os.Pipe()
	if err != nil {
		return -1, err
	}
	defer bridgeInput.Close()
	bridgeOutput, workerOutput, err := os.Pipe()
	if err != nil {
		return -1, err
	}
	defer bridgeOutput.Close()

	// Chrome owns this bridge. Only the worker owns the engine and resident policy.
	args := append(append([]string{}, os.Args[1:]...), workerArgument)
	command := exec.Command(executable, args...)
	command.Stdin, command.Stdout = workerInput, workerOutput
	configureWorker(command)
	if err = command.Start(); err != nil {
		return -1, err
	}
	_ = workerInput.Close()
	_ = workerOutput.Close()
	waited := make(chan int, 1)
	go func() {
		code := 0
		if waitErr := command.Wait(); waitErr != nil {
			if exitErr, ok := waitErr.(*exec.ExitError); ok {
				code = exitErr.ExitCode()
			} else {
				code = -1
			}
		}
		waited <- code
	}()
	go func() {
		_, _ = io.Copy(bridgeInput, input)
		_ = bridgeInput.Close()
	}()
	_, _ = io.Copy(output, bridgeOutput)
	// stdout 关闭有两种可能：worker 已退出（读取退出码，75 则重建），
	// 或常驻 worker 仍存活仅关闭了客户端流（保持旧语义：桥接退出、worker 存活）
	select {
	case code := <-waited:
		return code, nil
	case <-time.After(500 * time.Millisecond):
		return 0, nil
	}
}
