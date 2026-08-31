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

func DefaultRuntimeConfigPath() (string, error) {
	statePath, err := DefaultStatePath()
	if err != nil {
		return "", err
	}
	return filepath.Join(filepath.Dir(statePath), "runtime.yaml"), nil
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
	var snapshot Snapshot
	if err = json.Unmarshal(data, &snapshot); err != nil {
		return Snapshot{}, fmt.Errorf("decode model state: %w", err)
	}
	if snapshot.SchemaVersion != SchemaVersion || snapshot.Revision < 1 {
		return Snapshot{}, errors.New("unsupported model state schema")
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
