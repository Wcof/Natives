/**
 * tauri/proxy — Local Proxy domain facade（ADR-0020 / plan3 UI-001）。
 */

import { cmd } from './core';

export type PortMode = 'dynamic' | 'fixed';

export interface ProxySettings {
  id: string;
  enabledIntent: boolean;
  bindHost: string;
  portMode: PortMode;
  configuredPort: number;
  effectivePort: number;
  accessSecretRef: string;
  graceTimeoutMs: number;
  maxConcurrency: number;
  maxRequestBodyBytes: number;
  updatedAt: string;
}

export type ProxyRuntimeStatus =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'restarting'
  | 'stopping'
  | 'failed';

export interface ProxyEndpointInfo {
  protocol: string;
  path: string;
  url: string;
}

export interface ProxyStatusDTO {
  running: boolean;
  status: ProxyRuntimeStatus;
  host: string;
  port: number;
  effectivePort: number;
  startedAt?: string | null;
  uptimeSeconds: number;
  activeRequests: number;
  routeCount: number;
  engine: string;
  lastError?: string | null;
  protocolEndpoints: ProxyEndpointInfo[];
}

export type PoolPolicy = 'priority_round_robin' | 'round_robin' | 'least_inflight';

export type CredentialSelector =
  | { credential: { id: string } }
  | { pool: { policy: PoolPolicy } };

export interface RouteTarget {
  id: string;
  routeId: string;
  position: number;
  connectionId: string;
  modelId: string;
  credentialSelector: CredentialSelector;
  priority: number;
  enabled: boolean;
}

export interface Route {
  id: string;
  localModel: string;
  enabled: boolean;
  strategy: string;
  targets: RouteTarget[];
  createdAt: string;
  updatedAt: string;
}

export interface ProxyUsageRecord {
  id: string;
  routeId?: string | null;
  connectionId?: string | null;
  credentialId?: string | null;
  inboundProtocol: string;
  upstreamProtocol: string;
  localModel: string;
  upstreamModel: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  reasoningTokens?: number | null;
  cachedTokens?: number | null;
  latencyMs: number;
  status: string;
  errorCode?: string | null;
  createdAt: string;
}

export interface ProxyChatInput {
  protocol: 'anthropic_messages' | 'openai_chat_completions' | 'openai_responses';
  baseUrl: string;
  secretRef: string;
  model: string;
  requestJson: string;
}

export interface ProxyChatResult {
  ok: boolean;
  usageInputTokens: number;
  usageOutputTokens: number;
  stopReason: string;
  errorCategory?: string | null;
  errorCode?: string | null;
  errorMessage?: string | null;
}

export const proxyApi = {
  status: () => cmd<ProxyStatusDTO>('proxy_status'),
  start: () => cmd<boolean>('proxy_start'),
  stop: () => cmd<boolean>('proxy_stop'),
  restart: () => cmd<boolean>('proxy_restart'),

  getSettings: () => cmd<ProxySettings>('proxy_get_settings'),
  updateSettings: (input: {
    enabledIntent?: boolean;
    bindHost?: string;
    portMode?: string;
    configuredPort?: number;
    graceTimeoutMs?: number;
    maxConcurrency?: number;
    maxRequestBodyBytes?: number;
  }) => cmd<ProxySettings>('proxy_update_settings', { input }),

  listRoutes: () => cmd<Route[]>('proxy_list_routes'),
  getRoute: (id: string) => cmd<Route | null>('proxy_get_route', { id }),
  saveRoute: (route: Route) => cmd<Route>('proxy_save_route', { route }),
  deleteRoute: (id: string) => cmd<boolean>('proxy_delete_route', { id }),

  listUsageRecords: (limit?: number, offset?: number) =>
    cmd<ProxyUsageRecord[]>('proxy_list_usage_records', { limit, offset }),

  chat: (input: ProxyChatInput) => cmd<ProxyChatResult>('proxy_chat', { input }),
};

// 兼容别名
export const proxy = proxyApi;
export type ProxyStatusResult = ProxyStatusDTO;
