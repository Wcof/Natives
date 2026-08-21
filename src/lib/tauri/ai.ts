/**
 * tauri/ai — AI Resources domain facade（ADR-0020 / AIR-001..016）。
 */

import { cmd } from './core';

export interface Provider {
  id: string;
  presetName: string;
  apiProtocol: string;
  name: string;
  websiteUrl: string;
  baseUrl: string;
  createdAt: string;
  updatedAt: string;
}

export interface Connection {
  id: string;
  providerId: string;
  name: string;
  baseUrl: string;
  apiProtocol: string;
  proxyUrl?: string;
  projectId?: string;
}

export interface Credential {
  id: string;
  providerId: string;
  label: string;
  secretRef: string;
  maskedKey: string;
  isPrimary: boolean;
  isActive: boolean;
  status: string;
}

export interface Model {
  id: string;
  connectionId: string;
  modelId: string;
  family: string;
  displayName?: string;
}

export interface HealthCheckResult {
  reachable: boolean;
  error: string | null;
}

export const aiApi = {
  listProviders: () => cmd<Provider[]>('ai_list_providers'),
  getProvider: (id: string) => cmd<Provider | null>('ai_get_provider', { id }),
  listConnections: (providerId: string) => cmd<Connection[]>('ai_list_connections', { providerId }),
  listCredentials: (providerId: string) => cmd<Credential[]>('ai_list_credentials', { providerId }),
  createCredential: (input: { providerId: string; label: string; secret: string }) =>
    cmd<Credential>('ai_create_credential', { input }),
  deleteCredential: (input: { providerId: string; credentialId: string }) =>
    cmd<boolean>('ai_delete_credential', { input }),
  listModels: (connectionId: string) => cmd<Model[]>('ai_list_models', { connectionId }),
  checkHealth: (baseUrl: string) => cmd<HealthCheckResult>('ai_check_connection_health', { baseUrl }),
};
