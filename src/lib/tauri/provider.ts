/**
 * tauri/provider — Provider 域 facade（ARCH-002）
 *
 * provider / providerRouting 统一入口；wire-level（Stored/Host）类型与
 * normalize 纯函数只在本文件，不暴露到 barrel 之外。唯一 raw invoke 在 ./core.ts。
 */

import { cmd } from './core';
import type { NativesAPI } from './types';
import type { ProviderKeySummary, ProviderSummary, ProviderTestResult } from './types';
import { classifyError } from '../error-classifier';
import type { ProviderRouteBinding, ProviderRoutingSettings, ProviderRoutingApi } from '@/types/provider-routing';

interface StoredProviderKey extends Omit<ProviderKeySummary, 'lastError'> {
  lastErrorMessage: string | null;
}

interface StoredProvider {
  id: string;
  presetName: string;
  apiProtocol?: string;
  name: string;
  websiteUrl: string;
  baseUrl: string;
  defaultModel: string | null;
  primaryKeyId: string | null;
  keys: StoredProviderKey[];
  models?: Array<{ id: string; displayName?: string | null }>;
}

interface StoredProviderTestResult {
  success: boolean;
  error: string | null;
}

function normalizeProviderKey(key: StoredProviderKey): ProviderKeySummary {
  const { lastErrorMessage, ...rest } = key;
  return { ...rest, lastError: lastErrorMessage };
}

function normalizeProvider(provider: StoredProvider): ProviderSummary {
  return {
    id: provider.id,
    providerType: provider.presetName,
    apiProtocol: provider.apiProtocol ?? provider.presetName,
    displayName: provider.name,
    websiteUrl: provider.websiteUrl,
    baseUrl: provider.baseUrl,
    defaultModel: provider.defaultModel,
    primaryKeyId: provider.primaryKeyId,
    keys: provider.keys.map(normalizeProviderKey),
    models: provider.models ?? [],
  };
}

type HostRoutingSettings = { enabled: boolean; localEnabled: boolean; localPort: number; rectifier: { enabled?: boolean }; globalProxy: { enabled?: boolean; url?: string } };
type HostLocalRoutingTokenIssued = { token: string };
type HostRouteBinding = { id: string; position: number; providerId: string; credentialKind: 'api_key' | 'sub2api_pool'; credentialId: string | null; modelId: string; enabled: boolean; createdAt: string; updatedAt: string };

function normalizeRoutingSettings(settings: HostRoutingSettings): ProviderRoutingSettings {
  return {
    enabled: settings.enabled,
    loopbackEnabled: settings.localEnabled,
    loopbackPort: settings.localPort,
    rectifierEnabled: settings.rectifier.enabled === true,
    outboundProxyEnabled: settings.globalProxy.enabled === true,
    outboundProxyUrl: typeof settings.globalProxy.url === 'string' ? settings.globalProxy.url : null,
  };
}

function normalizeRouteBinding(binding: HostRouteBinding): ProviderRouteBinding {
  return {
    id: binding.id,
    providerId: binding.providerId,
    modelId: binding.modelId,
    credential: binding.credentialKind === 'api_key' && binding.credentialId ? { kind: 'api_key', keyId: binding.credentialId } : { kind: 'sub2api_pool' },
    priority: binding.position,
    enabled: binding.enabled,
  };
}

/**
 * Map a stored provider-test payload into UI-facing fields.
 * Non-throwing failures still need classification so AddProvider / ProviderDetail
 * do not dump raw diagnostic strings (protocol=/http_status=/request_id=…).
 */
export function normalizeProviderTest(result: StoredProviderTestResult): ProviderTestResult {
  if (result.success) {
    return {
      success: true,
      status: 'valid',
      testedAt: new Date().toISOString(),
      errorCode: null,
      userMessage: null,
    };
  }

  const raw = result.error ?? 'Provider test failed';
  const classified = classifyError(raw);
  const lower = raw.toLowerCase();
  const rateLimited =
    classified.category === 'RATE_LIMITED' ||
    lower.includes('http_status=429') ||
    lower.includes('rate limited') ||
    lower.includes('too many requests');

  return {
    success: false,
    status: rateLimited ? 'rate_limited' : 'invalid',
    testedAt: new Date().toISOString(),
    errorCode: rateLimited ? 'RATE_LIMITED' : classified.category,
    userMessage: classified.userMessage,
  };
}




  // Provider (unified API — single source of truth)
export const provider: NativesAPI['provider'] = {
    list: () => cmd<StoredProvider[]>('list_providers').then(providers => providers.map(normalizeProvider)),
    create: (input: { providerType: string; apiProtocol: string; displayName: string; websiteUrl: string; baseUrl: string; defaultModel: string; initialKey: { label: string; apiKey: string } }) =>
      cmd<StoredProvider>('add_provider', { input }).then(normalizeProvider),
    delete: (providerId: string) => cmd('delete_provider', { providerId }),
    updateDefaults: (input: { providerId: string; defaultModel: string }) =>
      cmd('provider_update_defaults', { input }),
    addKey: (input: { providerId: string; label: string; apiKey: string }) =>
      cmd<StoredProviderKey>('add_provider_key', { input }).then(normalizeProviderKey),
    testCandidate: (input: { providerType: string; apiProtocol?: string; baseUrl: string; apiKey: string; model: string }) =>
      cmd<StoredProviderTestResult>('test_provider_raw', { input }).then(normalizeProviderTest),
    testKey: (input: { providerId: string; keyId: string; model?: string }) =>
      cmd<StoredProviderTestResult>('provider_test', { input }).then(normalizeProviderTest),
    discoverModels: (input: { providerType: string; apiProtocol?: string; baseUrl: string; apiKey: string }) =>
      cmd<Array<{ id: string; displayName?: string }>>('provider_discover_models', { input }),
    discoverModelsSaved: (input: { providerId: string; keyId: string }) =>
      cmd<Array<{ id: string; displayName?: string }>>('provider_discover_models_saved', { input }),
    setPrimaryKey: (input: { providerId: string; keyId: string }) =>
      cmd('provider_set_primary_key', { input }),
    deleteKey: (input: { providerId: string; keyId: string }) =>
      cmd('delete_provider_key', { input }),
};

export const providerRouting: ProviderRoutingApi = {
    getSettings: () => cmd<HostRoutingSettings>('provider_routing_get_settings').then(normalizeRoutingSettings),
    saveSettings: (settings) => cmd<HostRoutingSettings>('provider_routing_update_settings', {
      input: {
        enabled: settings.enabled,
        localEnabled: settings.loopbackEnabled,
        localPort: settings.loopbackPort,
        rectifier: { enabled: settings.rectifierEnabled },
        globalProxy: { enabled: settings.outboundProxyEnabled, url: settings.outboundProxyUrl },
      },
    }).then(normalizeRoutingSettings),
    rotateLoopbackToken: () => cmd<HostLocalRoutingTokenIssued>('provider_routing_rotate_local_token').then((result) => result.token),
    listBindings: () => cmd<HostRouteBinding[]>('provider_routing_list_bindings').then((bindings) => bindings.map(normalizeRouteBinding)),
    saveBindings: (bindings) => cmd<HostRouteBinding[]>('provider_routing_update_bindings', {
      bindings: bindings.map((binding, position) => ({
        id: binding.id,
        position,
        providerId: binding.providerId,
        credentialKind: binding.credential.kind,
        credentialId: binding.credential.kind === 'api_key' ? binding.credential.keyId : null,
        modelId: binding.modelId,
        enabled: binding.enabled,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      })),
    }).then((bindings) => bindings.map(normalizeRouteBinding)),
    listSub2ApiAccounts: (providerId) => cmd<Array<{ id: string; providerId: string; name: string; platform: string; accountType: string; concurrency: number; priority: number; expiresAt: string | null; status: 'active' | 'paused' | 'expired' | 'invalid' }>>('provider_accounts_list', { providerId }).then((accounts) => accounts.map((account) => ({ ...account, email: null }))),
    previewSub2ApiImport: ({ providerId, content }) => cmd<{ accounts: Array<{ index: number; name: string; platform: string; accountType: string; action: 'create' | 'update' | 'skip' | 'reject'; error: string | null }> }>('provider_accounts_preview_import', { providerId, source: content }).then((preview) => ({
      items: preview.accounts.map((account) => ({ ...account, email: null, reason: account.error })),
      rejected: preview.accounts.filter((account) => account.action === 'reject').length,
    })),
    commitSub2ApiImport: ({ providerId, content }) => cmd<{ created: number; updated: number; skipped: number; failed: unknown[] }>('provider_accounts_commit_import', { request: { providerId, source: content } }).then((result) => ({ ...result, failed: result.failed.length })),
    deleteSub2ApiAccounts: ({ providerId, accountIds }) => cmd<{ deleted: string[]; notFound: string[]; failed: string[] }>('provider_accounts_batch_delete', { request: { providerId, accountIds } }),
    createSub2ApiPool: ({ name }) => cmd<string>('provider_accounts_create_pool', { input: { name } }),
};

