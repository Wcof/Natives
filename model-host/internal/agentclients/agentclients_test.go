package agentclients

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func tempHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
	return home
}

func TestDetectStatusReportsUninstalledClient(t *testing.T) {
	home := tempHome(t)
	if err := os.MkdirAll(filepath.Join(home, ".claude"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, ".claude", "settings.json"), []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}
	previousLookup, previousProbe := lookupHook, versionProbe
	lookupHook = func([]string) []string { return nil }
	versionProbe = func(string) (string, error) { return "", errorString("stub") }
	defer func() { lookupHook, versionProbe = previousLookup, previousProbe }()

	definition, ok := DefinitionByID("claude-code")
	if !ok {
		t.Fatal("claude-code definition missing")
	}
	status := DetectStatus(definition)
	if status.ID != "claude-code" || status.Name != "Claude Code" {
		t.Fatalf("unexpected identity: %+v", status)
	}
	if status.Installed {
		t.Fatal("a leftover config must not report the client as installed")
	}
	if !status.ConfigExists {
		t.Fatal("the leftover config should still be reported separately")
	}
	if status.ModificationState != "unconfigured" {
		t.Fatalf("expected unconfigured, got %q", status.ModificationState)
	}
	if !strings.Contains(status.ConfigPaths[0], home) {
		t.Fatalf("config path must live under home: %v", status.ConfigPaths)
	}
}

func TestRepeatedApplyPreservesOriginalBackup(t *testing.T) {
	home := tempHome(t)
	primary := filepath.Join(home, ".claude", "settings.json")
	if err := os.MkdirAll(filepath.Dir(primary), 0o755); err != nil {
		t.Fatal(err)
	}
	original := []byte("{\"original\":true}\n")
	if err := os.WriteFile(primary, original, 0o644); err != nil {
		t.Fatal(err)
	}
	for _, model := range []string{"first-model", "second-model"} {
		changes, _, err := BuildChanges("claude-code", home, "http://127.0.0.1:8317", "key", model, nil)
		if err != nil {
			t.Fatal(err)
		}
		if err := CommitTransaction("claude-code", primary, model, changes, nil); err != nil {
			t.Fatal(err)
		}
	}
	state, err := ReadStateFile(primary)
	if err != nil || state.ConfigurationRevision != 2 || len(state.BackupFiles) != 1 {
		t.Fatalf("unexpected repeated-apply state: %#v %v", state, err)
	}
	if _, err := CloseModification(primary); err != nil {
		t.Fatal(err)
	}
	restored, err := os.ReadFile(primary)
	if err != nil || string(restored) != string(original) {
		t.Fatalf("original config was not restored: %q %v", restored, err)
	}
}

func TestRefreshManagedUpdatesEndpointWithoutReplacingBackup(t *testing.T) {
	home := tempHome(t)
	changes, primary, err := BuildChanges("zcode", home, "http://127.0.0.1:8317", "key", "gemini-test", nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := CommitTransaction("zcode", primary, "gemini-test", changes, nil); err != nil {
		t.Fatal(err)
	}
	before, _ := ReadStateFile(primary)
	refreshed, err := RefreshManaged(home, "http://127.0.0.1:45678", "new-key")
	if err != nil || refreshed != 1 {
		t.Fatalf("refresh managed: %d %v", refreshed, err)
	}
	after, _ := ReadStateFile(primary)
	if after.ConfigurationRevision != 2 || len(after.BackupFiles) != len(before.BackupFiles) {
		t.Fatalf("managed journal was replaced: before=%#v after=%#v", before, after)
	}
	for index := range before.BackupFiles {
		if before.BackupFiles[index].BackupPath != after.BackupFiles[index].BackupPath {
			t.Fatal("refresh replaced the original backup")
		}
	}
	data, err := os.ReadFile(primary)
	if err != nil || !strings.Contains(string(data), "http://127.0.0.1:45678") {
		t.Fatalf("managed endpoint was not refreshed: %s %v", data, err)
	}
}

func TestClaudeCodeConfigRoundTripAndClose(t *testing.T) {
	home := tempHome(t)
	base, apiKey, model := "http://127.0.0.1:8317", "test-key", "gemini-test"
	changes, primary, err := BuildChanges("claude-code", home, base, apiKey, model, nil)
	if err != nil {
		t.Fatalf("BuildChanges: %v", err)
	}
	if len(changes) != 1 || changes[0].Path != filepath.Join(home, ".claude", "settings.json") {
		t.Fatalf("unexpected changes: %+v", changes)
	}
	if err := CommitTransaction("claude-code", primary, model, changes, nil); err != nil {
		t.Fatalf("CommitTransaction: %v", err)
	}
	data, err := os.ReadFile(primary)
	if err != nil {
		t.Fatalf("config not written: %v", err)
	}
	var document map[string]any
	if err := json.Unmarshal(data, &document); err != nil {
		t.Fatalf("config is not JSON: %v", err)
	}
	if document["model"] != model {
		t.Fatalf("selected model not applied: %v", document["model"])
	}
	if document["env"].(map[string]any)["ANTHROPIC_MODEL"] != model {
		t.Fatalf("ANTHROPIC_MODEL not applied")
	}
	env := document["env"].(map[string]any)
	if env["ANTHROPIC_BASE_URL"] != base || env["ANTHROPIC_AUTH_TOKEN"] != apiKey {
		t.Fatalf("gateway env not applied: %v", env)
	}
	if _, hasLegacy := env["ANTHROPIC_API_KEY"]; hasLegacy {
		t.Fatal("ANTHROPIC_API_KEY must be removed")
	}
	if _, err := ReadStateFile(primary); err != nil {
		t.Fatalf("state file unreadable: %v", err)
	}
	message, err := CloseModification(primary)
	if err != nil {
		t.Fatalf("CloseModification: %v", err)
	}
	if message == "" {
		t.Fatal("expected close message")
	}
	if _, err := os.Stat(primary); !os.IsNotExist(err) {
		t.Fatal("close must remove newly created config file")
	}
	if _, err := os.Stat(StatePath(primary)); !os.IsNotExist(err) {
		t.Fatal("close must remove state file")
	}
}

func TestBuildChangesWithModelsPublishesGatewayCatalog(t *testing.T) {
	home := tempHome(t)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
	t.Setenv("KIMI_CODE_HOME", filepath.Join(home, ".kimi-code"))
	t.Setenv("DSH_HOME", filepath.Join(home, ".dsh"))
	t.Setenv("HERMES_HOME", filepath.Join(home, ".hermes"))
	models := []ModelOption{
		{Name: "gemini-selected", Alias: "Selected", ContextWindow: 200000},
		{Name: "gemini-second", Alias: "Second", ContextWindow: 1000000},
	}
	for _, client := range []string{"zcode", "opencode", "openclaw", "kimi-code", "grok-build", "deepseek-harness", "hermes"} {
		changes, _, err := BuildChangesWithModels(client, home, "http://127.0.0.1:8317", "key", models[0].Name, models, nil)
		if err != nil {
			t.Fatalf("%s: %v", client, err)
		}
		var rendered strings.Builder
		for _, change := range changes {
			rendered.Write(change.Content)
		}
		if !strings.Contains(rendered.String(), models[0].Name) || !strings.Contains(rendered.String(), models[1].Name) {
			t.Fatalf("%s did not publish the full gateway catalog: %s", client, rendered.String())
		}
	}
}

func TestBuildersPreserveUnmanagedProviders(t *testing.T) {
	home := tempHome(t)
	t.Setenv("KIMI_CODE_HOME", filepath.Join(home, ".kimi-code"))
	fixtures := map[string]struct{ path, content string }{
		"codex":     {filepath.Join(home, ".codex", "config.toml"), "[model_providers.existing]\nname = 'Existing'\n"},
		"openclaw":  {filepath.Join(home, ".openclaw", "openclaw.json"), `{"models":{"providers":{"existing":{"api":"custom"}}}}`},
		"kimi-code": {filepath.Join(home, ".kimi-code", "config.toml"), "[providers.existing]\ntype = 'custom'\n"},
	}
	for client, fixture := range fixtures {
		if err := os.MkdirAll(filepath.Dir(fixture.path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(fixture.path, []byte(fixture.content), 0o644); err != nil {
			t.Fatal(err)
		}
		changes, _, err := BuildChangesWithModels(client, home, "http://127.0.0.1:8317", "key", "model", nil, nil)
		if err != nil {
			t.Fatalf("%s: %v", client, err)
		}
		if !strings.Contains(string(changes[0].Content), "existing") {
			t.Fatalf("%s removed an unmanaged provider: %s", client, changes[0].Content)
		}
	}
}

func TestZCodeWritesBothVariants(t *testing.T) {
	home := tempHome(t)
	changes, primary, err := BuildChanges("zcode", home, "http://127.0.0.1:8317", "k", "m1", nil)
	if err != nil {
		t.Fatalf("BuildChanges: %v", err)
	}
	if len(changes) != 2 {
		t.Fatalf("zcode must write app and cli configs, got %d", len(changes))
	}
	if primary != filepath.Join(home, ".zcode", "v2", "config.json") {
		t.Fatalf("unexpected primary: %s", primary)
	}
	for _, change := range changes {
		if err := os.MkdirAll(filepath.Dir(change.Path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(change.Path, change.Content, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	data, _ := os.ReadFile(changes[1].Path)
	if !strings.Contains(string(data), "model.main") && !strings.Contains(string(data), `"main"`) {
		t.Fatalf("cli config must set model.main: %s", data)
	}
	if strings.Contains(string(data), `"providers"`) || !strings.Contains(string(data), `"provider"`) {
		t.Fatalf("zcode config must use provider (singular): %s", data)
	}
	for _, change := range changes {
		var document map[string]any
		if err := json.Unmarshal(change.Content, &document); err != nil {
			t.Fatalf("decode zcode config: %v", err)
		}
		provider := document["provider"].(map[string]any)[ProviderID].(map[string]any)
		if provider["kind"] != "anthropic" || provider["defaultKind"] != "anthropic" || provider["apiFormat"] != "anthropic-messages" || provider["npm"] != "@ai-sdk/anthropic" {
			t.Fatalf("zcode provider protocol fields are incomplete: %#v", provider)
		}
	}
}

func TestFetchModelsParsesGatewayPayload(t *testing.T) {
	// parseModelOptions is exercised indirectly through parse; keep offline.
	options, err := parseModelOptions([]byte(`{"data":[{"id":"gemini-flash","display_name":"Gemini Flash","context_length":1000000}]}`))
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if len(options) != 1 || options[0].Name != "gemini-flash" || options[0].ContextWindow != 1000000 {
		t.Fatalf("unexpected options: %+v", options)
	}
}
