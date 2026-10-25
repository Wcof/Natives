package domain

import "time"

// SchemaVersion 4 removes the retired proxy gateway, access keys, routing
// settings and model aliases. Historical usage attribution is stored in the
// usage ledger, not in this configuration file.
const SchemaVersion = 4

type Protocol string

const (
	ProtocolOpenAIChat      Protocol = "openai_chat"
	ProtocolOpenAIResponses Protocol = "openai_responses"
	ProtocolAnthropic       Protocol = "anthropic_messages"
	ProtocolGemini          Protocol = "gemini"
)

type Model struct {
	ID            string `json:"id"`
	DisplayName   string `json:"displayName"`
	ContextLength int64  `json:"contextLength,omitempty"`
	Enabled       bool   `json:"enabled"`
	Manual        bool   `json:"manual"`
}

// Provider is an API-key connection used by internal AI calls. There is no
// OAuth inference provider and no model routing alias.
type Provider struct {
	ID             string   `json:"id"`
	Kind           string   `json:"kind"`
	Name           string   `json:"name"`
	BaseURL        string   `json:"baseUrl,omitempty"`
	Protocol       Protocol `json:"protocol,omitempty"`
	Enabled        bool     `json:"enabled"`
	AllowLAN       bool     `json:"allowLan"`
	SecretRef      string   `json:"secretRef,omitempty"`
	CredentialMask string   `json:"credentialMask,omitempty"`
	Models         []Model  `json:"models"`
	UpdatedAt      string   `json:"updatedAt"`
}

// Account is a legacy OAuth credential reference retained only for explicit
// quota inquiry. It never participates in inference or routing.
type Account struct {
	ID        string `json:"id"`
	Provider  string `json:"provider"`
	Label     string `json:"label"`
	Enabled   bool   `json:"enabled"`
	Status    string `json:"status"`
	SecretRef string `json:"secretRef"`
	Mask      string `json:"mask,omitempty"`
	UpdatedAt string `json:"updatedAt"`
}

type DefaultModelRef struct {
	ProviderID string `json:"providerId"`
	ModelID    string `json:"modelId"`
}

type Snapshot struct {
	SchemaVersion int             `json:"schemaVersion"`
	Revision      int64           `json:"revision"`
	Providers     []Provider      `json:"providers"`
	Accounts      []Account       `json:"accounts,omitempty"`
	DefaultModel  DefaultModelRef `json:"defaultModel,omitempty"`
	Resident      bool            `json:"resident"`
	UpdatedAt     string          `json:"updatedAt"`
}

func NewSnapshot() Snapshot {
	return Snapshot{
		SchemaVersion: SchemaVersion,
		Revision:      1,
		Providers:     []Provider{},
		Accounts:      []Account{},
		UpdatedAt:     time.Now().UTC().Format(time.RFC3339Nano),
	}
}
