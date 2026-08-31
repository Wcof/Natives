package secrets

import (
	"errors"
	"fmt"
	"strings"

	"github.com/zalando/go-keyring"
)

const serviceName = "com.natives.model-host"

var ErrNotFound = errors.New("secret not found")

type Store interface {
	Get(ref string) (string, error)
	Set(ref, value string) error
	Delete(ref string) error
}

type KeyringStore struct{}

func (KeyringStore) Get(ref string) (string, error) {
	if err := validateRef(ref); err != nil {
		return "", err
	}
	value, err := keyring.Get(serviceName, ref)
	if errors.Is(err, keyring.ErrNotFound) {
		return "", ErrNotFound
	}
	if err != nil {
		return "", fmt.Errorf("keychain unavailable: %w", err)
	}
	return value, nil
}

func (KeyringStore) Set(ref, value string) error {
	if err := validateRef(ref); err != nil {
		return err
	}
	if value == "" {
		return errors.New("secret value is required")
	}
	if err := keyring.Set(serviceName, ref, value); err != nil {
		return fmt.Errorf("keychain unavailable: %w", err)
	}
	return nil
}

func (KeyringStore) Delete(ref string) error {
	if err := validateRef(ref); err != nil {
		return err
	}
	if err := keyring.Delete(serviceName, ref); err != nil && !errors.Is(err, keyring.ErrNotFound) {
		return fmt.Errorf("keychain unavailable: %w", err)
	}
	return nil
}

func validateRef(ref string) error {
	ref = strings.TrimSpace(ref)
	if ref == "" || len(ref) > 180 || strings.ContainsAny(ref, "\r\n\x00") {
		return errors.New("invalid secret reference")
	}
	return nil
}

type MemoryStore struct {
	Values      map[string]string
	Unavailable bool
}

func NewMemoryStore() *MemoryStore { return &MemoryStore{Values: make(map[string]string)} }

func (s *MemoryStore) Get(ref string) (string, error) {
	if s.Unavailable {
		return "", errors.New("keychain unavailable")
	}
	value, ok := s.Values[ref]
	if !ok {
		return "", ErrNotFound
	}
	return value, nil
}

func (s *MemoryStore) Set(ref, value string) error {
	if s.Unavailable {
		return errors.New("keychain unavailable")
	}
	if err := validateRef(ref); err != nil {
		return err
	}
	if value == "" {
		return errors.New("secret value is required")
	}
	s.Values[ref] = value
	return nil
}

func (s *MemoryStore) Delete(ref string) error {
	if s.Unavailable {
		return errors.New("keychain unavailable")
	}
	delete(s.Values, ref)
	return nil
}
