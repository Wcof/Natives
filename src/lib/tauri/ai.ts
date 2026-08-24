/**
 * tauri/ai — AI Resources domain facade（ADR-0020 / plan3 UI-001）。
 */

import { cmd } from './core';

export interface Provider {
  id: string;
  presetKey?: string | null;
  name: string;
  websiteUrl: string;
  iconKey?: string | null;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export type UpstreamProtocol = 'openai_chat_completions' | 'openai_responses' | 'anthropic_messages';

export type ConnectionHealthStatus = 'healthy' | 'degraded' | 'unhealthy' | 'unknown';

export interface Connection {
  id: string;
  providerId: string;
  name: string;
  baseUrl: string;
  upstreamProtocol: UpstreamProtocol;
  modelsUrl?: string | null;
  proxyUrl?: string | null;
  headersJson?: string | null;
  enabled: boolean;
  healthStatus: ConnectionHealthStatus;
  lastCheckedAt?: string | null;
  createdAt: string;
  updatedAt: string;
}

export type CredentialKind = 'api_key' | 'oauth';

export type CredentialStatus =
  | 'active'
  | 'refreshing'
  | 'cooling'
  | 'reauth_required'
  | 'invalid'
  | 'disabled';

export interface Credential {
  id: string;
  providerId: string;
  kind: CredentialKind;
  label: string;
  secretRef: string;
  secretRevision: number;
  maskedIdentity: string;
  status: CredentialStatus;
  priority: number;
  concurrencyLimit: number;
  expiresAt?: string | null;
  lastRefreshedAt?: string | null;
  nextRefreshAt?: string | null;
  identityFingerprint?: string | null;
  metadataJson?: string | null;
  createdAt: string;
  updatedAt: string;
}

export type ModelSource = 'discovered' | 'oauth' | 'manual';
export type ModelAvailability = 'available' | 'unavailable' | 'unknown';

export interface Model {
  id: string;
  providerId?: string | null;
  connectionId?: string | null;
  sourceCredentialId?: string | null;
  modelId: string;
  displayName: string;
  source: ModelSource;
  capabilitiesJson?: string | null;
  availability: ModelAvailability;
  discoveredAt: string;
  lastSeenAt: string;
}

export interface DiscoveredModel {
  id: string;
  displayName: string;
  ownedBy?: string | null;
  source: ModelSource;
  availability: ModelAvailability;
  capabilities?: string | null;
}

export type QuotaStatus = 'available' | 'unknown' | 'stale' | 'error';

export interface QuotaWindow {
  id: string;
  snapshotId: string;
  label: string;
  remaining?: number | null;
  limitValue?: number | null;
  used?: number | null;
  unit?: string | null;
  resetAt?: string | null;
}

export interface QuotaSnapshot {
  id: string;
  credentialId: string;
  providerAdapter: string;
  status: QuotaStatus;
  planName?: string | null;
  errorCategory?: string | null;
  errorMessage?: string | null;
  fetchedAt: string;
  expiresAt?: string | null;
  windows: QuotaWindow[];
}

export interface AiResourcesSummary {
  providerCount: number;
  connectionCount: number;
  credentialCount: number;
  availableModelCount: number;
}

export interface DeleteImpact {
  connectionCount: number;
  credentialCount: number;
  modelCount: number;
  affectedRouteCount: number;
}

export type OauthFlowType = 'pkce' | 'device';

export interface OauthProviderPreset {
  providerId: string;
  name: string;
  platform: string;
  flow: OauthFlowType;
  authorizeUrl: string;
  tokenUrl: string;
  clientId: string;
  scopes: string[];
  deviceAuthorizeUrl: string;
  devicePollUrl: string;
  verificationUri: string;
  defaultBaseUrl: string;
  upstreamProtocol: string;
  defaultModels: [string, string][];
}

export type OauthSessionStatus =
  | 'starting'
  | 'waiting_for_user'
  | 'waiting_for_callback'
  | 'exchanging'
  | 'connected'
  | 'expired'
  | 'cancelled'
  | 'error';

export interface OauthSessionInfo {
  sessionId: string;
  providerId: string;
  flow: OauthFlowType;
  status: OauthSessionStatus;
  authorizeUrl?: string | null;
  userCode?: string | null;
  verificationUri?: string | null;
  expiresInSecs?: number | null;
  intervalSecs?: number | null;
  errorMessage?: string | null;
  connectedCredentialId?: string | null;
}

export interface HealthCheckResult {
  reachable: boolean;
  error: string | null;
}

export const aiApi = {
  // Providers
  listProviders: () => cmd<Provider[]>('ai_list_providers'),
  getProvider: (id: string) => cmd<Provider | null>('ai_get_provider', { id }),
  createProvider: (input: {
    id?: string;
    presetKey?: string;
    name: string;
    websiteUrl: string;
    iconKey?: string;
    enabled?: boolean;
  }) => cmd<Provider>('ai_create_provider', { input }),
  updateProvider: (input: {
    id: string;
    name?: string;
    websiteUrl?: string;
    iconKey?: string;
    enabled?: boolean;
  }) => cmd<Provider>('ai_update_provider', { input }),
  deleteProvider: (id: string) => cmd<boolean>('ai_delete_provider', { id }),
  getProviderDeleteImpact: (id: string) => cmd<DeleteImpact>('ai_get_provider_delete_impact', { id }),

  // Connections
  listConnections: (providerId?: string) => cmd<Connection[]>('ai_list_connections', { providerId }),
  getConnection: (id: string) => cmd<Connection | null>('ai_get_connection', { id }),
  createConnection: (input: {
    id?: string;
    providerId: string;
    name: string;
    baseUrl: string;
    upstreamProtocol: string;
    modelsUrl?: string;
    proxyUrl?: string;
    headersJson?: string;
    enabled?: boolean;
  }) => cmd<Connection>('ai_create_connection', { input }),
  updateConnection: (input: {
    id: string;
    name?: string;
    baseUrl?: string;
    upstreamProtocol?: string;
    modelsUrl?: string | null;
    proxyUrl?: string | null;
    headersJson?: string | null;
    enabled?: boolean;
  }) => cmd<Connection>('ai_update_connection', { input }),
  deleteConnection: (id: string) => cmd<boolean>('ai_delete_connection', { id }),

  // Credentials
  listCredentials: (providerId?: string) => cmd<Credential[]>('ai_list_credentials', { providerId }),
  getCredential: (id: string) => cmd<Credential | null>('ai_get_credential', { id }),
  createApiKeyCredential: (input: {
    providerId: string;
    label: string;
    apiKey: string;
    priority?: number;
    concurrencyLimit?: number;
    connectionIds?: string[];
  }) => cmd<Credential>('ai_create_api_key_credential', { input }),
  updateCredentialPolicy: (input: {
    id: string;
    label?: string;
    priority?: number;
    concurrencyLimit?: number;
    status?: string;
  }) => cmd<Credential>('ai_update_credential_policy', { input }),
  deleteCredential: (input: { providerId: string; credentialId: string }) =>
    cmd<boolean>('ai_delete_credential', { input }),

  // Models
  listModels: (connectionId?: string, credentialId?: string) =>
    cmd<Model[]>('ai_list_models', { connectionId, credentialId }),
  discoverModels: (input: {
    baseUrl: string;
    apiKey: string;
    modelsUrl?: string;
    headersJson?: string;
  }) => cmd<DiscoveredModel[]>('ai_discover_models', { input }),
  confirmDiscoveredModels: (input: {
    providerId?: string;
    connectionId?: string;
    models: DiscoveredModel[];
  }) => cmd<Model[]>('ai_confirm_discovered_models', { input }),
  createManualModel: (input: {
    providerId?: string;
    connectionId?: string;
    modelId: string;
    displayName: string;
    capabilitiesJson?: string;
  }) => cmd<Model>('ai_create_manual_model', { input }),
  deleteModel: (id: string) => cmd<boolean>('ai_delete_model', { id }),

  // OAuth
  oauthListPresets: () => cmd<OauthProviderPreset[]>('ai_oauth_list_presets'),
  oauthStart: (input: { providerId: string; accountLabel?: string }) =>
    cmd<OauthSessionInfo>('ai_oauth_start', { input }),
  oauthPoll: (sessionId: string) => cmd<OauthSessionInfo>('ai_oauth_poll', { sessionId }),
  oauthSubmitCallback: (callbackUrl: string) =>
    cmd<OauthSessionInfo>('ai_oauth_submit_callback', { callbackUrl }),
  oauthCancel: (sessionId: string) => cmd<boolean>('ai_oauth_cancel', { sessionId }),
  oauthRefresh: (credentialId: string) => cmd<Credential>('ai_oauth_refresh', { credentialId }),

  // Quota & Summary
  getQuota: (credentialId: string) => cmd<QuotaSnapshot | null>('ai_get_quota', { credentialId }),
  getSummary: () => cmd<AiResourcesSummary>('ai_get_summary'),
  checkHealth: (baseUrl: string) => cmd<HealthCheckResult>('ai_check_connection_health', { baseUrl }),
};
