export type ProviderReadiness = 'no_provider' | 'no_model' | 'ready';
export interface ProviderInfo { id: string; provider_type: string; display_name: string; api_base_url?: string; health_status?: string; default_model?: string | null; has_active_key?: boolean; models?: Array<{ id: string; display_name?: string | null; context_window?: number }>; }
export function classifyProviderReadiness(p: ProviderInfo[]): ProviderReadiness { if (p.length === 0) return 'no_provider'; return p.some(x => x.models?.length && x.models.length > 0 && x.has_active_key) ? 'ready' : 'no_model'; }
export function selectAssistantModel(p: ProviderInfo[]): { providerId: string; modelId: string } | null { for (const x of p) { if (!x.has_active_key) continue; if (!x.models?.length) continue; const m = x.default_model ?? x.models[0]?.id; if (m) return { providerId: x.id, modelId: m }; } return null; }
export function canTestDiscoveredModel(_m: unknown[]): boolean { return true; }
export function connectionFingerprint(b: string, k: string): string { return b + '|' + k.slice(0, 8); }
export function normalizeDiscoveredModels<T extends { id: string }>(m: T[]): T[] { return m.filter(x => x.id.trim().length > 0); }
export function selectDiscoveredModel<T extends { id: string }>(m: T[]): T | null { return m[0] ?? null; }
