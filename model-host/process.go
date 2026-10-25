package main

import (
	"io"
	"os"
	"os/exec"
	"time"

	"github.com/ldh/natives/model-host/internal/singleinstance"
)

const workerArgument = "--model-host-worker"

func bridgeModelHost(dir string, input io.Reader, output io.Writer) error {
	if connection, err := singleinstance.Connect(dir); err == nil {
		relayNativeMessages(connection, input, output)
		return nil
	}
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	// Chrome input has one reader for the entire bridge lifetime. The same pipe
	// survives worker replacement, so a retired copy loop cannot steal frames.
	workerInput, bridgeInput, err := os.Pipe()
	if err != nil {
		return err
	}
	defer workerInput.Close()
	defer bridgeInput.Close()
	go func() {
		_, _ = io.Copy(bridgeInput, input)
		_ = bridgeInput.Close()
	}()
	_, err = spawnWorkerAndRelay(executable, workerInput, output)
	return err
}

func spawnWorkerAndRelay(executable string, workerInput *os.File, output io.Writer) (int, error) {
	bridgeOutput, workerOutput, err := os.Pipe()
	if err != nil {
		return -1, err
	}
	defer bridgeOutput.Close()
	defer workerOutput.Close()

	// Chrome owns this bridge. Only the worker owns the engine and resident policy.
	args := append(append([]string{}, os.Args[1:]...), workerArgument)
	command := exec.Command(executable, args...)
	command.Stdin, command.Stdout = workerInput, workerOutput
	configureWorker(command)
	if err = command.Start(); err != nil {
		return -1, err
	}
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
	_, _ = io.Copy(output, bridgeOutput)
	// stdout 关闭有两种可能：worker 已退出，或常驻 worker 仍存活仅关闭了
	// 客户端流（桥接退出、worker 存活）。
	select {
	case code := <-waited:
		return code, nil
	case <-time.After(500 * time.Millisecond):
		return 0, nil
	}
}
