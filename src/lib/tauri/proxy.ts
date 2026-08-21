/**
 * tauri/proxy — Local Proxy domain facade（ADR-0020 / PRX-001..015）。
 */

import { cmd } from './core';

export interface ProxyStatusResult {
  running: boolean;
  port: number;
  host: string;
  uptimeSeconds: number;
  protocolSupport: string[];
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
  errorCategory: string | null;
  errorCode: string | null;
  errorMessage: string | null;
}

export const proxy = {
  status: () => cmd<ProxyStatusResult>('proxy_status'),
  start: () => cmd<boolean>('proxy_start'),
  stop: () => cmd<boolean>('proxy_stop'),
  chat: (input: ProxyChatInput) => cmd<ProxyChatResult>('proxy_chat', { input }),
};
