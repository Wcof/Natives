/**
 * tauri/types-provider — Provider / credential domain 共享类型（ARCH-002 split）
 *
 * Provider / API key 等凭证侧 wire 类型声明于此；provider facade
 * （./provider.ts）与业务组件从这里取类型。
 */

export interface ProviderKeySummary {
  id: string;
  providerId: string;
  label: string;
  maskedKey: string;
  isPrimary: boolean;
  isActive: boolean;
  status: 'untested' | 'valid' | 'invalid' | 'rate_limited' | 'unavailable';
  lastTestedAt: string | null;
  lastError: string | null;
  createdAt: string;
}

export interface ProviderSummary {
  id: string;
  providerType: string;
  apiProtocol: string;
  displayName: string;
  websiteUrl: string;
  baseUrl: string;
  defaultModel: string | null;
  primaryKeyId: string | null;
  keys: ProviderKeySummary[];
  models?: Array<{ id: string; displayName?: string | null }>;
}

export interface ProviderTestResult {
  success: boolean;
  status: ProviderKeySummary['status'];
  testedAt: string;
  errorCode: string | null;
  userMessage: string | null;
}
