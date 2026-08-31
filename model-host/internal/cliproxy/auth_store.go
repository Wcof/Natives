package cliproxy

import (
	"context"
	"encoding/json"
	"errors"
	"sort"
	"sync"

	"github.com/ldh/natives/model-host/internal/secrets"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
)

// AuthStore is the sole persistence adapter used by CLIProxyAPI. Auth payloads
// live in OS Keychain; the non-secret ID index is supplied by the Model Host.
type AuthStore struct {
	mu      sync.Mutex
	secrets secrets.Store
	ids     []string
	onIDs   func([]string) error
	onSave  func(*coreauth.Auth) error
}

func (s *AuthStore) SetOnSave(callback func(*coreauth.Auth) error) { s.onSave = callback }

func NewAuthStore(secretStore secrets.Store, ids []string, onIDs func([]string) error) *AuthStore {
	return &AuthStore{secrets: secretStore, ids: append([]string(nil), ids...), onIDs: onIDs}
}

func (s *AuthStore) List(context.Context) ([]*coreauth.Auth, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	result := make([]*coreauth.Auth, 0, len(s.ids))
	for _, id := range s.ids {
		raw, err := s.secrets.Get(authRef(id))
		if errors.Is(err, secrets.ErrNotFound) {
			continue
		}
		if err != nil {
			return nil, err
		}
		var auth coreauth.Auth
		if err = json.Unmarshal([]byte(raw), &auth); err != nil {
			return nil, errors.New("stored credential is invalid")
		}
		result = append(result, &auth)
	}
	return result, nil
}

func (s *AuthStore) Save(_ context.Context, auth *coreauth.Auth) (string, error) {
	if auth == nil || auth.ID == "" {
		return "", errors.New("credential id is required")
	}
	raw, err := json.Marshal(auth)
	if err != nil {
		return "", errors.New("credential serialization failed")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	previous, previousErr := s.secrets.Get(authRef(auth.ID))
	if previousErr != nil && !errors.Is(previousErr, secrets.ErrNotFound) {
		return "", previousErr
	}
	if err = s.secrets.Set(authRef(auth.ID), string(raw)); err != nil {
		return "", err
	}
	if !contains(s.ids, auth.ID) {
		next := append(append([]string(nil), s.ids...), auth.ID)
		sort.Strings(next)
		if s.onIDs != nil {
			if err = s.onIDs(next); err != nil {
				_ = s.secrets.Delete(authRef(auth.ID))
				return "", err
			}
		}
		s.ids = next
	}
	if s.onSave != nil {
		if err = s.onSave(auth.Clone()); err != nil {
			if previousErr == nil {
				_ = s.secrets.Set(authRef(auth.ID), previous)
			} else {
				_ = s.secrets.Delete(authRef(auth.ID))
			}
			return "", err
		}
	}
	return auth.ID, nil
}

func (s *AuthStore) Delete(_ context.Context, id string) error {
	if id == "" {
		return errors.New("credential id is required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.secrets.Delete(authRef(id)); err != nil {
		return err
	}
	next := make([]string, 0, len(s.ids))
	for _, current := range s.ids {
		if current != id {
			next = append(next, current)
		}
	}
	if s.onIDs != nil {
		if err := s.onIDs(next); err != nil {
			return err
		}
	}
	s.ids = next
	return nil
}

func authRef(id string) string { return "oauth:" + id }

func contains(values []string, value string) bool {
	for _, current := range values {
		if current == value {
			return true
		}
	}
	return false
}
