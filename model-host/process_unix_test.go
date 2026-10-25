//go:build darwin || linux

package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/user"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func TestModelHostProcessHelper(t *testing.T) {
	if os.Getenv("NATIVES_MODEL_HOST_RESTART_TEST") == "1" {
		writer := nativeio.NewWriter(os.Stdout)
		_ = writer.Write(nativeio.Response{OK: true, Event: "ready"})
		for {
			request, err := nativeio.Read(os.Stdin)
			if err != nil {
				os.Exit(0)
			}
			_ = writer.Write(nativeio.Response{ID: request.ID, OK: true})
		}
	}
	if os.Getenv("NATIVES_MODEL_HOST_PROCESS_TEST") != "1" {
		return
	}
	store := secrets.NewMemoryStore()
	if ref, value := os.Getenv("NATIVES_MODEL_HOST_PROCESS_SECRET_REF"), os.Getenv("NATIVES_MODEL_HOST_PROCESS_SECRET_VALUE"); ref != "" && value != "" {
		store.Values[ref] = value
	}
	if ref := os.Getenv("NATIVES_MODEL_HOST_PROCESS_REAL_SECRET_REF"); ref != "" {
		isolatedHome, homeSet := os.LookupEnv("HOME")
		account, lookupErr := user.Current()
		if lookupErr != nil {
			os.Exit(2)
		}
		_ = os.Setenv("HOME", account.HomeDir)
		value, err := (secrets.KeyringStore{}).Get(ref)
		if homeSet {
			_ = os.Setenv("HOME", isolatedHome)
		} else {
			_ = os.Unsetenv("HOME")
		}
		if err != nil {
			fmt.Fprintln(os.Stderr, "read-only upstream credential unavailable")
			os.Exit(2)
		}
		store.Values[ref] = value
	}
	run(store)
	os.Exit(0)
}

func TestBridgeForwardsChromeInputOnce(t *testing.T) {
	t.Setenv("NATIVES_MODEL_HOST_RESTART_TEST", "1")
	inputReader, input, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer inputReader.Close()
	defer input.Close()
	output, outputWriter, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer output.Close()
	defer outputWriter.Close()
	originalArgs := os.Args
	os.Args = []string{originalArgs[0], "-test.run=^TestModelHostProcessHelper$", "--"}
	defer func() { os.Args = originalArgs }()
	done := make(chan error, 1)
	go func() { done <- bridgeModelHost(t.TempDir(), inputReader, outputWriter) }()
	output.SetReadDeadline(time.Now().Add(5 * time.Second))
	if response := readResponse(t, output); response.Event != "ready" {
		t.Fatal("worker not ready")
	}
	writeRequest(t, input, nativeio.Request{ID: "first", Method: "snapshot"})
	if response := readResponse(t, output); response.ID != "first" {
		t.Fatal("bridge lost a Chrome request")
	}
	input.Close()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("bridge did not exit after EOF")
	}
}

func TestResidentHostSurvivesNativeEOF(t *testing.T) {
	dir := t.TempDir()
	first := startProcessClient(t, dir)
	snapshot := first.call(t, "model_host_set_resident", `{"expectedRevision":1,"resident":true}`)
	workerPID := singleChildPID(t, first.cmd.Process.Pid)
	t.Cleanup(func() { _ = syscall.Kill(workerPID, syscall.SIGTERM) })
	_ = first.input.Close()
	first.wait(t)
	assertProcessRunning(t, workerPID)

	second := startProcessClient(t, dir)
	reopened := second.call(t, "model_snapshot", `{}`)
	if !reopened.Resident || reopened.Revision != snapshot.Revision {
		t.Fatalf("reopening saw a different authority: %#v", reopened)
	}
	if children := childProcessIDs(t, second.cmd.Process.Pid); len(children) != 0 {
		t.Fatalf("relay spawned a replacement worker: %v", children)
	}
	second.call(t, "model_host_set_resident", fmt.Sprintf(`{"expectedRevision":%d,"resident":false}`, reopened.Revision))
	_ = second.input.Close()
	second.wait(t)
	waitForProcessExit(t, workerPID)
}

func TestNonResidentHostExitsAfterLastClient(t *testing.T) {
	dir := t.TempDir()
	first := startProcessClient(t, dir)
	_ = first.call(t, "model_snapshot", `{}`)
	workerPID := singleChildPID(t, first.cmd.Process.Pid)
	t.Cleanup(func() { _ = syscall.Kill(workerPID, syscall.SIGTERM) })
	_ = first.input.Close()
	first.wait(t)
	waitForProcessExit(t, workerPID)
}

func TestNativeRelayCompletesRequestBeforeEOF(t *testing.T) {
	dir := t.TempDir()
	first := startProcessClient(t, dir)
	snapshot := first.call(t, "model_host_set_resident", `{"expectedRevision":1,"resident":true}`)
	workerPID := singleChildPID(t, first.cmd.Process.Pid)
	t.Cleanup(func() { _ = syscall.Kill(workerPID, syscall.SIGTERM) })
	relay := startProcessClient(t, dir)
	_ = relay.output.SetReadDeadline(time.Now().Add(2 * time.Second))
	writeRequest(t, relay.input, nativeio.Request{ID: "last-request", Method: "model_snapshot", Params: json.RawMessage(`{}`)})
	_ = relay.input.Close()
	response := readResponse(t, relay.output)
	if !response.OK || response.ID != "last-request" || decodeSnapshot(t, response.Result).Revision != snapshot.Revision {
		t.Fatal("relay discarded the response to a request received before EOF")
	}
	relay.wait(t)
	second := startProcessClient(t, dir)
	current := second.call(t, "model_snapshot", `{}`)
	second.call(t, "model_host_set_resident", fmt.Sprintf(`{"expectedRevision":%d,"resident":false}`, current.Revision))
	_ = second.input.Close()
	second.wait(t)
	_ = first.input.Close()
	first.wait(t)
	waitForProcessExit(t, workerPID)
}

type processClient struct {
	cmd    *exec.Cmd
	input  io.WriteCloser
	output *os.File
	done   <-chan error
	nextID int
}

func startProcessClient(t *testing.T, dir string) *processClient {
	t.Helper()
	cmd := exec.Command(os.Args[0], "-test.run=^TestModelHostProcessHelper$", "--")
	cmd.Env = append(os.Environ(), "NATIVES_MODEL_HOST_PROCESS_TEST=1", "NATIVES_MODEL_HOST_CONFIG_DIR="+dir)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	childInput, input, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer childInput.Close()
	t.Cleanup(func() { _ = input.Close() })
	output, childOutput, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer childOutput.Close()
	t.Cleanup(func() { _ = output.Close() })
	cmd.Stdin, cmd.Stdout = childInput, childOutput
	if err = cmd.Start(); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	client := &processClient{cmd: cmd, input: input, output: output, done: done}
	t.Cleanup(func() {
		_ = input.Close()
		_ = output.Close()
		_ = cmd.Process.Kill()
		for _, pid := range childProcessIDs(t, cmd.Process.Pid) {
			_ = syscall.Kill(pid, syscall.SIGTERM)
		}
	})
	return client
}

func (c *processClient) call(t *testing.T, method, params string) domain.Snapshot {
	t.Helper()
	c.nextID++
	id := fmt.Sprintf("process-%d", c.nextID)
	if err := c.output.SetReadDeadline(time.Now().Add(5 * time.Second)); err != nil {
		t.Fatal(err)
	}
	writeRequest(t, c.input, nativeio.Request{ID: id, Method: method, Params: json.RawMessage(params)})
	for {
		response := readResponse(t, c.output)
		if response.ID != id {
			continue
		}
		if !response.OK {
			t.Fatalf("%s failed: %s", method, response.ErrorCode)
		}
		return decodeSnapshot(t, response.Result)
	}
}

func (c *processClient) wait(t *testing.T) {
	t.Helper()
	select {
	case <-c.done:
	case <-time.After(3 * time.Second):
		t.Fatal("Native Messaging process did not exit")
	}
}

func childProcessIDs(t *testing.T, parent int) []int {
	t.Helper()
	output, err := exec.Command("pgrep", "-P", strconv.Itoa(parent)).Output()
	if err != nil {
		return nil
	}
	var ids []int
	for _, field := range strings.Fields(string(output)) {
		if value, err := strconv.Atoi(field); err == nil {
			ids = append(ids, value)
		}
	}
	return ids
}

func singleChildPID(t *testing.T, parent int) int {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if ids := childProcessIDs(t, parent); len(ids) == 1 {
			return ids[0]
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("expected exactly one model worker child for %d", parent)
	return 0
}

func assertProcessRunning(t *testing.T, pid int) {
	t.Helper()
	if err := syscall.Kill(pid, 0); err != nil {
		t.Fatalf("resident worker %d is not running: %v", pid, err)
	}
}

func waitForProcessExit(t *testing.T, pid int) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		if syscall.Kill(pid, 0) == syscall.ESRCH {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("non-resident worker %d did not exit", pid)
}
