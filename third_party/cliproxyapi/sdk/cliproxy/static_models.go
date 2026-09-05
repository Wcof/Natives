package cliproxy

import (
	"strings"

	"github.com/router-for-me/CLIProxyAPI/v7/internal/registry"
)

// StaticModelDefinitions returns static model definitions for a given channel/provider.
func StaticModelDefinitions(provider string) []*ModelInfo {
	return registry.GetStaticModelDefinitionsByChannel(strings.ToLower(strings.TrimSpace(provider)))
}
