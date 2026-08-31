package domain

import (
	"os"
	"path/filepath"
	"testing"
)

func TestRepositoryCreatesAndRevisionChecksAtomicState(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nested", "state.json")
	repo := NewRepository(path)
	initial, err := repo.Load()
	if err != nil {
		t.Fatal(err)
	}
	if initial.Revision != 1 || len(initial.Providers) != len(OAuthProviders) {
		t.Fatalf("unexpected initial snapshot: %#v", initial)
	}
	expected := initial.Revision
	updated, err := repo.Update(&expected, func(snapshot *Snapshot) error {
		snapshot.Gateway.Resident = true
		return nil
	})
	if err != nil || updated.Revision != 2 || !updated.Gateway.Resident {
		t.Fatalf("unexpected update: %#v %v", updated, err)
	}
	if _, err = repo.Update(&expected, func(*Snapshot) error { return nil }); err == nil {
		t.Fatal("stale revision was accepted")
	}
	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm()&0o077 != 0 {
		t.Fatalf("state file permissions are not private: %v %v", info, err)
	}
}
