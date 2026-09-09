//go:build darwin || linux

package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func TestModelHostProcessHelper(t *testing.T) {
	if os.Getenv("NATIVES_MODEL_HOST_PROCESS_TEST") != "1" {
		return
	}
	store := secrets.NewMemoryStore()
	store.Values["gateway:access-key"] = "process-test-key"
	store.Values["process-provider"] = "process-upstream-key"
	run(store)
	os.Exit(0)
}

func TestResidentGatewaySurvivesBrowserProcessTermination(t *testing.T) {
	for _, termination := range []struct {
		name   string
		signal syscall.Signal
	}{
		{"graceful", syscall.SIGTERM},
		{"forced", syscall.SIGKILL},
	} {
		t.Run(termination.name, func(t *testing.T) {
			dir := t.TempDir()
			complete := make(chan struct{})
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", "text/event-stream")
				_, _ = io.WriteString(w, "data: {\"id\":\"stream\",\"object\":\"chat.completion.chunk\",\"model\":\"process-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"before-close\"},\"finish_reason\":null}]}\n\n")
				w.(http.Flusher).Flush()
				select {
				case <-complete:
					_, _ = io.WriteString(w, "data: {\"id\":\"stream\",\"object\":\"chat.completion.chunk\",\"model\":\"process-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"after-close\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
				case <-r.Context().Done():
				}
			}))
			t.Cleanup(upstream.Close)
			initial, err := domain.NewRepository(filepath.Join(dir, "state.json")).Update(nil, func(s *domain.Snapshot) error {
				s.Providers = append(s.Providers, domain.Provider{ID: "process-provider", Kind: "custom", Name: "Process test", BaseURL: upstream.URL + "/v1", Protocol: domain.ProtocolOpenAIChat, Enabled: true, SecretRef: "process-provider", Models: []domain.Model{{ID: "process-model", Enabled: true}}})
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
			browser := startProcessClient(t, dir)
			snapshot := browser.call(t, "model_gateway_set_resident", fmt.Sprintf(`{"expectedRevision":%d,"resident":true}`, initial.Revision))
			snapshot = browser.call(t, "model_gateway_start", fmt.Sprintf(`{"expectedRevision":%d}`, snapshot.Revision))
			assertGatewayResponds(t, snapshot.Gateway.BaseURL)
			workerPID := snapshot.Gateway.PID
			t.Cleanup(func() { _ = syscall.Kill(workerPID, syscall.SIGTERM) })
			request, err := http.NewRequest(http.MethodPost, snapshot.Gateway.BaseURL+"/v1/chat/completions", strings.NewReader(`{"model":"process-model","messages":[{"role":"user","content":"hello"}],"stream":true}`))
			if err != nil {
				t.Fatal(err)
			}
			request.Header.Set("Authorization", "Bearer process-test-key")
			request.Header.Set("Content-Type", "application/json")
			stream, err := (&http.Client{Timeout: 5 * time.Second}).Do(request)
			if err != nil {
				t.Fatal(err)
			}
			defer stream.Body.Close()

			// Chromium can terminate the Native Messaging process group after closing its pipes.
			if err := syscall.Kill(-browser.cmd.Process.Pid, termination.signal); err != nil {
				t.Fatal(err)
			}
			browser.wait(t)
			assertGatewayResponds(t, snapshot.Gateway.BaseURL)
			close(complete)
			body, err := io.ReadAll(stream.Body)
			if err != nil || stream.StatusCode != http.StatusOK || !strings.Contains(string(body), "after-close") || !strings.Contains(string(body), "[DONE]") {
				t.Fatalf("browser termination interrupted the in-flight stream: status=%d, error=%v", stream.StatusCode, err)
			}

			reopened := startProcessClient(t, dir)
			snapshot = reopened.call(t, "model_snapshot", `{}`)
			if snapshot.Gateway.PID != workerPID || snapshot.Gateway.State != "running" {
				t.Fatalf("reopening replaced the resident gateway: pid=%d, state=%s", snapshot.Gateway.PID, snapshot.Gateway.State)
			}
			snapshot = reopened.call(t, "model_gateway_stop", fmt.Sprintf(`{"expectedRevision":%d}`, snapshot.Revision))
			if snapshot.Gateway.State != "stopped" {
				t.Fatal("explicit stop did not stop the gateway")
			}
			reopened.call(t, "model_gateway_set_resident", fmt.Sprintf(`{"expectedRevision":%d,"resident":false}`, snapshot.Revision))
			_ = reopened.input.Close()
			reopened.wait(t)
			waitForProcessExit(t, workerPID)
		})
	}
}

func TestGatewayFollowsLastNonResidentPage(t *testing.T) {
	dir := t.TempDir()
	first := startProcessClient(t, dir)
	snapshot := first.call(t, "model_gateway_start", `{"expectedRevision":1}`)
	workerPID := snapshot.Gateway.PID
	t.Cleanup(func() { _ = syscall.Kill(workerPID, syscall.SIGTERM) })
	second := startProcessClient(t, dir)
	if other := second.call(t, "model_snapshot", `{}`); other.Gateway.PID != workerPID {
		t.Fatal("two pages started separate gateways")
	}
	_ = first.input.Close()
	first.wait(t)
	assertGatewayResponds(t, snapshot.Gateway.BaseURL)
	_ = second.input.Close()
	second.wait(t)
	waitForProcessExit(t, workerPID)
}

func TestResidentGatewaySurvivesNativeEOF(t *testing.T) {
	dir := t.TempDir()
	first := startProcessClient(t, dir)
	snapshot := first.call(t, "model_gateway_set_resident", `{"expectedRevision":1,"resident":true}`)
	snapshot = first.call(t, "model_gateway_start", fmt.Sprintf(`{"expectedRevision":%d}`, snapshot.Revision))
	workerPID := snapshot.Gateway.PID
	t.Cleanup(func() { _ = syscall.Kill(workerPID, syscall.SIGTERM) })
	_ = first.input.Close()
	first.wait(t)
	assertGatewayResponds(t, snapshot.Gateway.BaseURL)
	second := startProcessClient(t, dir)
	snapshot = second.call(t, "model_snapshot", `{}`)
	if snapshot.Gateway.PID != workerPID {
		t.Fatal("reopening replaced the resident worker")
	}
	second.call(t, "model_gateway_set_resident", fmt.Sprintf(`{"expectedRevision":%d,"resident":false}`, snapshot.Revision))
	_ = second.input.Close()
	second.wait(t)
	waitForProcessExit(t, workerPID)
}

func TestNativeRelayCompletesRequestBeforeEOF(t *testing.T) {
	dir := t.TempDir()
	first := startProcessClient(t, dir)
	snapshot := first.call(t, "model_gateway_start", `{"expectedRevision":1}`)
	workerPID := snapshot.Gateway.PID
	t.Cleanup(func() { _ = syscall.Kill(workerPID, syscall.SIGTERM) })
	relay := startProcessClient(t, dir)
	_ = relay.output.SetReadDeadline(time.Now().Add(2 * time.Second))
	writeRequest(t, relay.input, nativeio.Request{ID: "last-request", Method: "model_snapshot", Params: json.RawMessage(`{}`)})
	_ = relay.input.Close()
	response := readResponse(t, relay.output)
	if !response.OK || response.ID != "last-request" || decodeSnapshot(t, response.Result).Gateway.PID != workerPID {
		t.Fatal("relay discarded the response to a request received before EOF")
	}
	relay.wait(t)
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
		// A failed assertion must not leave a detached test gateway behind.
		data, readErr := os.ReadFile(filepath.Join(dir, "state.json"))
		var snapshot domain.Snapshot
		if readErr == nil && json.Unmarshal(data, &snapshot) == nil && snapshot.Gateway.PID > 0 {
			_ = syscall.Kill(snapshot.Gateway.PID, syscall.SIGTERM)
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
	case <-time.After(2 * time.Second):
		t.Fatal("Native Messaging process did not exit")
	}
}

func assertGatewayResponds(t *testing.T, baseURL string) {
	t.Helper()
	client := &http.Client{Timeout: time.Second}
	request, err := http.NewRequest(http.MethodGet, baseURL+"/v1/models", nil)
	if err != nil {
		t.Fatal(err)
	}
	request.Header.Set("Authorization", "Bearer process-test-key")
	response, err := client.Do(request)
	if err != nil {
		t.Fatalf("resident gateway stopped after browser disconnect: %v", err)
	}
	defer response.Body.Close()
	_, _ = io.Copy(io.Discard, response.Body)
	if response.StatusCode != http.StatusOK {
		t.Fatalf("gateway status = %d", response.StatusCode)
	}
}

func waitForProcessExit(t *testing.T, pid int) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if syscall.Kill(pid, 0) == syscall.ESRCH {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("non-resident worker %d did not exit", pid)
}
