'use client';

/**
 * capability-secrets — 连接器凭证与 OAuth 的唯一前端入口（ADR-0016 决策 7）
 *
 * 规则与 files-api 相同：组件禁止直接触碰 `window.nativesAPI`；本模块把
 * 「可能不存在」收敛为显式可用性检查 + 统一错误。凭证明文只在 set 的入参里
 * 短暂存在，list 永不返回明文；OAuth token 由 Host/daemon 持有，前端不落任何。
 */

import type { NativesAPI } from '@/lib/tauri-adapter';

export class CapabilitySecretsUnavailableError extends Error {
  constructor(section: string) {
    super(`[capability-secrets] nativesAPI.${section} not available (Tauri IPC required)`);
    this.name = 'CapabilitySecretsUnavailableError';
  }
}

function section<K extends keyof NativesAPI>(key: K): NativesAPI[K] {
  const api = typeof window === 'undefined' ? null : window.nativesAPI?.[key];
  if (!api) throw new CapabilitySecretsUnavailableError(String(key));
  return api;
}

/** 凭证/OAuth 能力是否可用（浏览器 dev 模式为 false） */
export function hasCapabilitySecrets(): boolean {
  return typeof window !== 'undefined' && !!window.nativesAPI?.capabilitySecret;
}

export type CapabilitySecretKind = 'mcp_env' | 'mcp_bearer' | 'mcp_oauth_refresh';

export interface CapabilitySecretEntry {
  id: string;
  kind: string;
  keyName: string | null;
  createdAt: string;
}

export function listCapabilitySecrets(ownerRef: string): Promise<CapabilitySecretEntry[]> {
  return section('capabilitySecret').list(ownerRef);
}

export async function createCapabilitySecret(data: {
  kind: CapabilitySecretKind;
  ownerRef: string;
  keyName?: string;
  plaintext: string;
}): Promise<string> {
  const { id } = await section('capabilitySecret').set(data);
  return id;
}

/** 发起浏览器 OAuth 授权（Host loopback + PKCE）。resolve 即成功；取消/超时/失败均 reject。 */
export function startMcpOauth(data: {
  serverId: string;
  authorizeUrl: string;
  tokenUrl: string;
  clientId: string;
  scopes?: string[];
}): Promise<{ ok: boolean; hasRefresh: boolean }> {
  return section('mcpOauth').start(data);
}
