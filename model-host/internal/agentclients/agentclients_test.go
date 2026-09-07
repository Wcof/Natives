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
		t.Fatal("empty home must not report installed")
	}
	if status.ModificationState != "unconfigured" {
		t.Fatalf("expected unconfigured, got %q", status.ModificationState)
	}
	if !strings.Contains(status.ConfigPaths[0], home) {
		t.Fatalf("config path must live under home: %v", status.ConfigPaths)
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
	if document["model"] != "claude-sonnet-4-6" {
		t.Fatalf("sonnet mapping not applied: %v", document["model"])
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
