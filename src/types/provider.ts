// ── Provider/Vendor Types for Settings → 供应商管理 ──

/** 预设供应商模板（来自 cc-switch 的数据子集） */
export interface ProviderPreset {
  name: string;
  /** Only protocols implemented by the provider backend may be configured. */
  protocol?: ApiProtocol;
  /** 中文名（用于 locale=zh 时显示） */
  nameZh?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  baseUrl: string;
  /** 简短描述（英文） */
  description?: string;
  /** 简短描述（中文） */
  descriptionZh?: string;
  category?: 'official' | 'cn_official' | 'aggregator' | 'third_party';
  icon?: string;
  iconColor?: string;
}

/**
 * 供应商 API 协议（上游请求格式）。
 * 前端新增流程仅暴露后端 test/discover 已支持的子集；
 * gemini / ollama 保留在类型中供运行时/历史数据兼容，暂不在新增 UI 中配置。
 */
export type ApiProtocol =
  | 'openai_chat_completions'
  | 'openai_responses'
  | 'anthropic_messages'
  | 'gemini_generate_content'
  | 'ollama_chat';

/** 单条 API Key — 返回给前端的始终是脱敏版本 */
export interface ProviderKey {
  id: string;
  providerId: string;
  label: string;
  /** 脱敏后的 Key（如 "sk-a…1b2c"），绝不含完整 Key */
  maskedKey: string;
  status: 'unknown' | 'valid' | 'invalid' | 'rate_limited';
  lastTestedAt: string | null;
  lastError: string | null;
  createdAt: string;
}

/** 用户已保存的供应商（存入 SQLite） */
export interface UserProvider {
  id: string;
  presetName: string;
  name: string;
  websiteUrl: string;
  baseUrl: string;
  /** 多个 API Key */
  keys: ProviderKey[];
  createdAt: string;
  updatedAt: string;
}

// ── Unified types ──

export type ProviderKeyStatus = 'untested' | 'valid' | 'invalid' | 'rate_limited' | 'unavailable';

export interface ProviderKeySummary {
  id: string; providerId: string; label: string; maskedKey: string;
  isActive: boolean; isPrimary: boolean; status: ProviderKeyStatus;
  lastTestedAt: string | null; lastError: string | null; createdAt: string;
}

export interface ProviderSummary {
  id: string; providerType: string; apiProtocol: ApiProtocol; displayName: string; websiteUrl: string; baseUrl: string;
  defaultModel: string | null; primaryKeyId: string | null; keys: ProviderKeySummary[];
  models?: Array<{ id: string; displayName?: string | null }>;
}

export interface CreateProviderInput {
  providerType: string; apiProtocol: ApiProtocol; displayName: string; websiteUrl: string; baseUrl: string;
  defaultModel: string | null; initialKey?: { label: string; apiKey: string } | null;
}

export interface UpdateDefaultsInput { providerId: string; defaultModel: string | null; }
export interface AddKeyInput { providerId: string; label: string; apiKey: string; }
export interface TestKeyInput { providerId: string; keyId: string; }
export interface TestKeyResult { success: boolean; status: ProviderKeyStatus; testedAt: string; errorCode: string | null; userMessage: string | null; }
export interface SetPrimaryKeyInput { providerId: string; keyId: string; }
export interface DeleteKeyInput { providerId: string; keyId: string; }
