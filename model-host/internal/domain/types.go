package domain

import "time"

const SchemaVersion = 3

const (
	DefaultRequestRetry            = 3
	DefaultMaxRetryIntervalSeconds = 30
)

var OAuthProviders = []string{"codex", "claude", "antigravity", "kimi", "xai"}

type Protocol string

const (
	ProtocolOpenAIChat      Protocol = "openai_chat"
	ProtocolOpenAIResponses Protocol = "openai_responses"
	ProtocolAnthropic       Protocol = "anthropic_messages"
	ProtocolGemini          Protocol = "gemini"
)

type Model struct {
	ID            string   `json:"id"`
	DisplayName   string   `json:"displayName"`
	Alias         string   `json:"alias,omitempty"`
	ContextLength int64    `json:"contextLength,omitempty"`
	AccountIDs    []string `json:"accountIds,omitempty"`
	Enabled       bool     `json:"enabled"`
	Manual        bool     `json:"manual"`
}

type Provider struct {
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
	Models         []Model  `json:"models"`
	UpdatedAt      string   `json:"updatedAt"`
}

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

type GatewayAccessKey struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	SecretRef string `json:"secretRef"`
	Mask      string `json:"mask"`
	Enabled   bool   `json:"enabled"`
	CreatedAt string `json:"createdAt"`
	UpdatedAt string `json:"updatedAt"`
}

type GatewaySettings struct {
	PreferredPort             int    `json:"preferredPort,omitempty"`
	ProxyURL                  string `json:"proxyUrl,omitempty"`
	ProxyCredentialRef        string `json:"proxyCredentialRef,omitempty"`
	RoutingStrategy           string `json:"routingStrategy,omitempty"` // "round_robin" | "fill_first"
	SessionAffinity           bool   `json:"sessionAffinity"`
	SessionAffinityTTL        int    `json:"sessionAffinityTtl,omitempty"`
	RequestRetry              int    `json:"requestRetry,omitempty"`
	MaxRetryCredentials       int    `json:"maxRetryCredentials,omitempty"`
	MaxRetryIntervalSeconds   int    `json:"maxRetryIntervalSeconds,omitempty"`
	StreamingBootstrapRetries int    `json:"streamingBootstrapRetries,omitempty"`
	StartOnSettingsOpen       bool   `json:"startOnSettingsOpen"`
}

type Gateway struct {
	State         string             `json:"state"`
	Resident      bool               `json:"resident"`
	PreferredPort int                `json:"preferredPort,omitempty"`
	Port          int                `json:"port,omitempty"`
	BaseURL       string             `json:"baseUrl,omitempty"`
	ErrorCode     string             `json:"errorCode,omitempty"`
	AccessKeyMask string             `json:"accessKeyMask,omitempty"`
	AccessKeys    []GatewayAccessKey `json:"accessKeys,omitempty"`
	Settings      GatewaySettings    `json:"settings"`
	PID                 int                `json:"pid,omitempty"`
	KernelVersion       string             `json:"kernelVersion,omitempty"`
	LatestKernelVersion string             `json:"latestKernelVersion,omitempty"`
	Version             string             `json:"version,omitempty"`
}

type Snapshot struct {
	SchemaVersion int        `json:"schemaVersion"`
	Revision      int64      `json:"revision"`
	Providers     []Provider `json:"providers"`
	Accounts      []Account  `json:"accounts"`
	Gateway       Gateway    `json:"gateway"`
	UpdatedAt     string     `json:"updatedAt"`
}

func NewSnapshot() Snapshot {
	now := time.Now().UTC().Format(time.RFC3339Nano)
	providers := make([]Provider, 0, len(OAuthProviders))
	for _, key := range OAuthProviders {
		providers = append(providers, Provider{
			ID: key, Kind: "oauth", OAuthProvider: key, Name: key,
			Enabled: true, Models: []Model{}, UpdatedAt: now,
		})
	}
	defaultKey := GatewayAccessKey{
		ID:        "key_default",
		Name:      "默认密钥",
		SecretRef: "gateway:access-key",
		Enabled:   true,
		CreatedAt: now,
		UpdatedAt: now,
	}
	return Snapshot{
		SchemaVersion: SchemaVersion,
		Revision:      1,
		Providers:     providers,
		Accounts:      []Account{},
		Gateway: Gateway{
			State:      "stopped",
			AccessKeys: []GatewayAccessKey{defaultKey},
			Settings: GatewaySettings{
				RoutingStrategy:         "round_robin",
				RequestRetry:            DefaultRequestRetry,
				MaxRetryIntervalSeconds: DefaultMaxRetryIntervalSeconds,
			},
		},
		UpdatedAt: now,
	}
}
