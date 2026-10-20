package host

import (
	"context"
	"encoding/json"
	"path/filepath"
	"testing"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func TestRunningGatewayKeyMutationsComplete(t *testing.T) {
	dir := t.TempDir()
	engine, err := NewEngine(domain.NewRepository(filepath.Join(dir, "state.json")), secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	engine.runtimeConfigPath = filepath.Join(dir, "runtime.yaml")
	running, err := engine.startGateway(context.Background(), json.RawMessage(`{"expectedRevision":1}`))
	if err != nil {
		t.Fatal(err)
	}
	created, err := engine.createGatewayKey(json.RawMessage(`{"expectedRevision":` + jsonNumber(running.Revision) + `,"name":"Running key"}`))
	if err != nil {
		t.Fatal(err)
	}
	snapshot := created["snapshot"].(domain.Snapshot)
	if snapshot.Gateway.State != "running" || len(snapshot.Gateway.AccessKeys) != len(running.Gateway.AccessKeys)+1 {
		t.Fatal("key creation did not reconfigure the running gateway")
	}
	key := snapshot.Gateway.AccessKeys[len(snapshot.Gateway.AccessKeys)-1]
	updated, err := engine.updateGatewayKey(json.RawMessage(`{"expectedRevision":` + jsonNumber(snapshot.Revision) + `,"keyId":"` + key.ID + `","enabled":false}`))
	if err != nil {
		t.Fatal(err)
	}
	deleted, err := engine.deleteGatewayKey(json.RawMessage(`{"expectedRevision":` + jsonNumber(updated.Revision) + `,"keyId":"` + key.ID + `"}`))
	if err != nil {
		t.Fatal(err)
	}
	if deleted.Gateway.State != "running" || len(deleted.Gateway.AccessKeys) != len(running.Gateway.AccessKeys) {
		t.Fatal("key deletion did not reconfigure the running gateway")
	}
}
