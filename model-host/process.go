package main

import (
	"io"
	"os"
	"os/exec"

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
	workerInput, bridgeInput, err := os.Pipe()
	if err != nil {
		return err
	}
	defer workerInput.Close()
	defer bridgeInput.Close()
	bridgeOutput, workerOutput, err := os.Pipe()
	if err != nil {
		return err
	}
	defer bridgeOutput.Close()
	defer workerOutput.Close()

	// Chrome owns this bridge. Only the worker owns the engine and resident policy.
	args := append(append([]string{}, os.Args[1:]...), workerArgument)
	command := exec.Command(executable, args...)
	command.Stdin, command.Stdout = workerInput, workerOutput
	configureWorker(command)
	if err = command.Start(); err != nil {
		return err
	}
	_ = workerInput.Close()
	_ = workerOutput.Close()
	go func() { _ = command.Wait() }()
	go func() {
		_, _ = io.Copy(bridgeInput, input)
		_ = bridgeInput.Close()
	}()
	_, _ = io.Copy(output, bridgeOutput)
	return nil
}
