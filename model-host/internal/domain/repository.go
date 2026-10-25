package domain

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
)

type Repository struct {
	path string
	mu   sync.Mutex
}

func NewRepository(path string) *Repository { return &Repository{path: path} }

func DefaultStatePath() (string, error) {
	if override := os.Getenv("NATIVES_MODEL_HOST_CONFIG_DIR"); override != "" {
		if !filepath.IsAbs(override) {
			return "", errors.New("model host config override must be absolute")
		}
		return filepath.Join(override, "state.json"), nil
	}
	dir, err := os.UserConfigDir()
	if err != nil {
		return "", fmt.Errorf("resolve config directory: %w", err)
	}
	return filepath.Join(dir, "Natives", "model-host", "state.json"), nil
}

func (r *Repository) Load() (Snapshot, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.loadUnlocked()
}

func (r *Repository) Update(expected *int64, mutate func(*Snapshot) error) (Snapshot, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	snapshot, err := r.loadUnlocked()
	if err != nil {
		return Snapshot{}, err
	}
	if expected != nil && *expected != snapshot.Revision {
		return Snapshot{}, fmt.Errorf("revision_conflict")
	}
	if err = mutate(&snapshot); err != nil {
		return Snapshot{}, err
	}
	snapshot.SchemaVersion = SchemaVersion
	snapshot.Revision++
	snapshot.UpdatedAt = time.Now().UTC().Format(time.RFC3339Nano)
	if err = r.saveUnlocked(snapshot); err != nil {
		return Snapshot{}, err
	}
	return snapshot, nil
}

func (r *Repository) loadUnlocked() (Snapshot, error) {
	data, err := os.ReadFile(r.path)
	if errors.Is(err, os.ErrNotExist) {
		initial := NewSnapshot()
		if err = r.saveUnlocked(initial); err != nil {
			return Snapshot{}, err
		}
		return initial, nil
	}
	if err != nil {
		return Snapshot{}, fmt.Errorf("read model state: %w", err)
	}
	var header struct {
		SchemaVersion int `json:"schemaVersion"`
	}
	if err = json.Unmarshal(data, &header); err != nil {
		return Snapshot{}, fmt.Errorf("decode model state: %w", err)
	}
	if header.SchemaVersion < 1 {
		return Snapshot{}, errors.New("unsupported model state schema")
	}
	if header.SchemaVersion == SchemaVersion {
		var snapshot Snapshot
		if err = json.Unmarshal(data, &snapshot); err != nil {
			return Snapshot{}, fmt.Errorf("decode model state: %w", err)
		}
		if snapshot.Revision < 1 {
			return Snapshot{}, errors.New("unsupported model state schema")
		}
		return snapshot, nil
	}
	if header.SchemaVersion > SchemaVersion {
		return Snapshot{}, errors.New("unsupported model state schema")
	}
	var legacy legacySnapshot
	if err = json.Unmarshal(data, &legacy); err != nil {
		return Snapshot{}, fmt.Errorf("decode legacy model state: %w", err)
	}
	if legacy.Revision < 1 {
		return Snapshot{}, errors.New("unsupported model state schema")
	}
	snapshot := legacy.migrate()
	// The migration is monotonic and idempotent: the first successful load
	// persists schema 4; later loads take the fast path above.
	if err = r.saveUnlocked(snapshot); err != nil {
		return Snapshot{}, fmt.Errorf("migrate model state: %w", err)
	}
	return snapshot, nil
}

func (r *Repository) saveUnlocked(snapshot Snapshot) error {
	dir := filepath.Dir(r.path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return fmt.Errorf("create model state directory: %w", err)
	}
	data, err := json.MarshalIndent(snapshot, "", "  ")
	if err != nil {
		return fmt.Errorf("encode model state: %w", err)
	}
	tmp, err := os.CreateTemp(dir, ".state-*.tmp")
	if err != nil {
		return fmt.Errorf("create model state temp file: %w", err)
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName)
	if err = tmp.Chmod(0o600); err == nil {
		_, err = tmp.Write(append(data, '\n'))
	}
	if err == nil {
		err = tmp.Sync()
	}
	closeErr := tmp.Close()
	if err == nil {
		err = closeErr
	}
	if err != nil {
		return fmt.Errorf("write model state: %w", err)
	}
	if err = os.Rename(tmpName, r.path); err != nil {
		return fmt.Errorf("replace model state: %w", err)
	}
	if directory, openErr := os.Open(dir); openErr == nil {
		_ = directory.Sync()
		_ = directory.Close()
	}
	return nil
}

// legacySnapshot decodes schema 1-3. Its gateway, access keys and aliases are
// read only to preserve the user resident choice; they are never re-emitted.
type legacySnapshot struct {
	SchemaVersion int              `json:"schemaVersion"`
	Revision      int64            `json:"revision"`
	Providers     []legacyProvider `json:"providers"`
	Accounts      []Account        `json:"accounts"`
	Gateway       legacyGateway    `json:"gateway"`
	DefaultModel  DefaultModelRef  `json:"defaultModel"`
	UpdatedAt     string           `json:"updatedAt"`
}

type legacyProvider struct {
	ID             string   `json:"id"`
	Kind           string   `json:"kind"`
	OAuthProvider  string   `json:"oauthProvider,omitempty"`
	Name           string   `json:"name"`
	BaseURL        string   `json:"baseUrl,omitempty"`
	Protocol       Protocol `json:"protocol,omitempty"`
	Enabled        bool     `json:"enabled"`
	AllowLAN       bool     `json:"allowLan"`
	SecretRef      string   `json:"secretRef,omitempty"`
	CredentialMask string   `json:"credentialMask,omitempty"`
	Models         []struct {
		ID            string   `json:"id"`
		DisplayName   string   `json:"displayName"`
		Alias         string   `json:"alias,omitempty"`
		ContextLength int64    `json:"contextLength,omitempty"`
		AccountIDs    []string `json:"accountIds,omitempty"`
		Enabled       bool     `json:"enabled"`
		Manual        bool     `json:"manual"`
	} `json:"models"`
	UpdatedAt string `json:"updatedAt"`
}

type legacyGateway struct {
	Resident bool `json:"resident"`
}

func (legacy legacySnapshot) migrate() Snapshot {
	snapshot := Snapshot{
		SchemaVersion: SchemaVersion,
		Revision:      legacy.Revision + 1,
		Providers:     make([]Provider, 0, len(legacy.Providers)),
		Accounts:      append([]Account(nil), legacy.Accounts...),
		DefaultModel:  legacy.DefaultModel,
		Resident:      legacy.Gateway.Resident,
		UpdatedAt:     time.Now().UTC().Format(time.RFC3339Nano),
	}
	for _, old := range legacy.Providers {
		if old.Kind != "custom" {
			// OAuth providers served the retired routing kernel. Their Keychain
			// entries and account metadata remain for quota inquiry only.
			continue
		}
		provider := Provider{
			ID: old.ID, Kind: "custom", Name: old.Name, BaseURL: old.BaseURL,
			Protocol: old.Protocol, Enabled: old.Enabled, AllowLAN: old.AllowLAN,
			SecretRef: old.SecretRef, CredentialMask: old.CredentialMask,
			Models: make([]Model, 0, len(old.Models)), UpdatedAt: old.UpdatedAt,
		}
		for _, model := range old.Models {
			provider.Models = append(provider.Models, Model{
				ID: model.ID, DisplayName: model.DisplayName,
				ContextLength: model.ContextLength, Enabled: model.Enabled, Manual: model.Manual,
			})
		}
		snapshot.Providers = append(snapshot.Providers, provider)
	}
	if snapshot.DefaultModel.ProviderID != "" {
		valid := false
		for _, provider := range snapshot.Providers {
			if provider.ID != snapshot.DefaultModel.ProviderID {
				continue
			}
			for _, model := range provider.Models {
				if model.ID == snapshot.DefaultModel.ModelID {
					valid = true
					break
				}
			}
		}
		if !valid {
			snapshot.DefaultModel = DefaultModelRef{}
		}
	}
	return snapshot
}
