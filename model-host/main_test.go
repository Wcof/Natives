package main

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"io"
	"net"
	"path/filepath"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/host"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func TestNativeClientReconnectsToSameResidentEngine(t *testing.T) {
	engine, err := host.NewEngine(domain.NewRepository(filepath.Join(t.TempDir(), "state.json")), secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer engine.Close()
	hub := &clientHub{writers: make(map[*nativeio.Writer]bool), idle: make(chan struct{}, 1)}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	firstClient, firstHost := net.Pipe()
	hub.serve(ctx, engine, firstHost, firstHost, firstHost.Close)
	writeRequest(t, firstClient, nativeio.Request{ID: "resident", Method: "model_gateway_set_resident", Params: json.RawMessage(`{"expectedRevision":1,"resident":true}`)})
	firstResponse := readResponse(t, firstClient)
	firstSnapshot := decodeSnapshot(t, firstResponse.Result)
	if !firstSnapshot.Gateway.Resident {
		t.Fatal("resident setting was not stored")
	}
	_ = firstClient.Close()
	select {
	case <-hub.idle:
	case <-time.After(time.Second):
		t.Fatal("client disconnect was not observed")
	}

	secondClient, secondHost := net.Pipe()
	hub.serve(ctx, engine, secondHost, secondHost, secondHost.Close)
	writeRequest(t, secondClient, nativeio.Request{ID: "snapshot", Method: "model_snapshot", Params: json.RawMessage(`{}`)})
	secondSnapshot := decodeSnapshot(t, readResponse(t, secondClient).Result)
	_ = secondClient.Close()
	if secondSnapshot.Revision != firstSnapshot.Revision || !secondSnapshot.Gateway.Resident {
		t.Fatalf("reconnected client saw different authority: %#v", secondSnapshot)
	}
}

func writeRequest(t *testing.T, writer io.Writer, request nativeio.Request) {
	t.Helper()
	payload, _ := json.Marshal(request)
	if err := binary.Write(writer, binary.LittleEndian, uint32(len(payload))); err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write(payload); err != nil {
		t.Fatal(err)
	}
}

func readResponse(t *testing.T, reader io.Reader) nativeio.Response {
	t.Helper()
	var size uint32
	if err := binary.Read(reader, binary.LittleEndian, &size); err != nil {
		t.Fatal(err)
	}
	payload := make([]byte, size)
	if _, err := io.ReadFull(reader, payload); err != nil {
		t.Fatal(err)
	}
	var response nativeio.Response
	if err := json.Unmarshal(payload, &response); err != nil {
		t.Fatal(err)
	}
	return response
}

func decodeSnapshot(t *testing.T, value any) domain.Snapshot {
	t.Helper()
	payload, _ := json.Marshal(value)
	var snapshot domain.Snapshot
	if err := json.Unmarshal(payload, &snapshot); err != nil {
		t.Fatal(err)
	}
	return snapshot
}
