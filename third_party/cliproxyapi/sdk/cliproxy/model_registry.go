package cliproxy

import (
	"strings"

	"github.com/router-for-me/CLIProxyAPI/v7/internal/registry"
)

// ModelInfo re-exports the registry model info structure.
type ModelInfo = registry.ModelInfo

// ModelRegistryHook re-exports the registry hook interface for external integrations.
type ModelRegistryHook = registry.ModelRegistryHook

// ModelRegistry describes registry operations consumed by external callers.
type ModelRegistry interface {
	RegisterClient(clientID, clientProvider string, models []*ModelInfo)
	UnregisterClient(clientID string)
	SetModelQuotaExceeded(clientID, modelID string)
	ClearModelQuotaExceeded(clientID, modelID string)
	ClientSupportsModel(clientID, modelID string) bool
	GetAvailableModels(handlerType string) []map[string]any
	GetAvailableModelsByProvider(provider string) []*ModelInfo
}

// GlobalModelRegistry returns the shared registry instance.
func GlobalModelRegistry() ModelRegistry {
	return registry.GetGlobalRegistry()
}

// SetGlobalModelRegistryHook registers an optional hook on the shared global registry instance.
func SetGlobalModelRegistryHook(hook ModelRegistryHook) {
	registry.GetGlobalRegistry().SetHook(hook)
}

// StaticModelDefinitions returns the kernel's built-in OAuth model catalog.
func StaticModelDefinitions(provider string) []*ModelInfo {
	switch strings.ToLower(strings.TrimSpace(provider)) {
	case "antigravity":
		return registry.GetAntigravityModels()
	case "claude":
		return registry.GetClaudeModels()
	case "codex":
		return registry.GetCodexProModels()
	case "kimi":
		return registry.GetKimiModels()
	case "xai":
		return registry.GetXAIModels()
	default:
		return nil
	}
}
