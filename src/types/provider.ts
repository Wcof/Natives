// ── Provider/Vendor Types for Settings → 供应商管理 ──

/** 预设供应商模板（来自 cc-switch 的数据子集） */
export interface ProviderPreset {
  name: string;
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

/** 单条 API Key */
export interface ProviderKey {
  id: string;
  providerId: string;
  label: string;
  apiKey: string;
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
