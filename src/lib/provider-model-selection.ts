export type ProviderReadiness = 'no_provider' | 'no_model' | 'ready';

export interface ProviderInfo {
  id: string;
  provider_type: string;
  display_name: string;
  api_base_url?: string;
  health_status?: string;
  default_model?: string | null;
  has_active_key?: boolean;
  models?: Array<{ id: string; display_name?: string | null; context_window?: number }>;
}

/** Wire shape used by the assistant model picker. */
export interface AssistantProviderOption {
  id: string;
  name: string;
  presetName: string;
  baseUrl: string;
  keys: Array<{ id: string; label: string; maskedKey: string }>;
  models: Array<{ id: string; displayName?: string }>;
  defaultModel?: string | null;
}

/**
 * Accept both production RPC envelopes `{ providers: [...] }` and fixture arrays.
 * Without this, a successful list response is treated as "no providers".
 */
export function unwrapProviderListPayload(payload: unknown): Array<Record<string, unknown>> {
  if (Array.isArray(payload)) {
    return payload.filter((item): item is Record<string, unknown> => !!item && typeof item === 'object');
  }
  if (payload && typeof payload === 'object') {
    const rec = payload as Record<string, unknown>;
    if (Array.isArray(rec.providers)) {
      return rec.providers.filter(
        (item): item is Record<string, unknown> => !!item && typeof item === 'object',
      );
    }
  }
  return [];
}

/** Collapse repeated model rows (cache can hold the same model_id twice). */
export function dedupeModelsById<T extends { id: string; displayName?: string }>(
  models: T[],
): T[] {
  const byId = new Map<string, T>();
  for (const model of models) {
    const key = model.id.trim().toLowerCase();
    if (!key) continue;
    const existing = byId.get(key);
    if (!existing) {
      byId.set(key, model);
      continue;
    }
    // Prefer the entry that carries a distinct display name.
    const existingLabel = (existing.displayName ?? existing.id).trim();
    const nextLabel = (model.displayName ?? model.id).trim();
    if (
      (!existing.displayName || existingLabel === existing.id) &&
      model.displayName &&
      nextLabel !== model.id
    ) {
      byId.set(key, model);
    }
  }
  return Array.from(byId.values());
}

/**
 * Prefer the richer of two provider rows with the same id (or same name+baseUrl).
 * Keeps the one with an active key and more discovered models.
 */
function preferProvider(
  a: AssistantProviderOption,
  b: AssistantProviderOption,
): AssistantProviderOption {
  const score = (p: AssistantProviderOption) =>
    (p.keys.length > 0 ? 1000 : 0) + p.models.length + (p.defaultModel ? 1 : 0);
  return score(b) > score(a) ? b : a;
}

export function mapWireProviders(payload: unknown): AssistantProviderOption[] {
  const mapped = unwrapProviderListPayload(payload)
    .map((p) => {
      const name = String(p.display_name ?? p.displayName ?? p.name ?? p.id ?? '');
      const id = String(p.id ?? '');
      const hasActiveKey = Boolean(p.has_active_key ?? p.hasActiveKey ?? false);
      const models = Array.isArray(p.models)
        ? dedupeModelsById(
            (p.models as Array<Record<string, unknown>>)
              .map((m) => ({
                id: String(m.id ?? '').trim(),
                displayName: String(m.display_name ?? m.displayName ?? m.id ?? ''),
              }))
              .filter((m) => m.id.length > 0),
          )
        : [];
      return {
        id,
        name,
        presetName: String(p.provider_type ?? p.presetName ?? p.preset_name ?? name),
        baseUrl: String(p.api_base_url ?? p.baseUrl ?? p.base_url ?? ''),
        keys: hasActiveKey ? [{ id: 'active', label: 'default', maskedKey: '••••' }] : [],
        models,
        defaultModel:
          typeof p.default_model === 'string'
            ? p.default_model
            : typeof p.defaultModel === 'string'
              ? p.defaultModel
              : null,
      };
    })
    .filter((p) => p.id.length > 0);

  // Dedupe by id first, then collapse same display_name + baseUrl ghosts from dual mirrors.
  const byId = new Map<string, AssistantProviderOption>();
  for (const provider of mapped) {
    const existing = byId.get(provider.id);
    byId.set(provider.id, existing ? preferProvider(existing, provider) : provider);
  }

  const byIdentity = new Map<string, AssistantProviderOption>();
  for (const provider of byId.values()) {
    const identity = `${provider.name.trim().toLowerCase()}|${provider.baseUrl
      .trim()
      .toLowerCase()
      .replace(/\/+$/, '')}`;
    const existing = byIdentity.get(identity);
    byIdentity.set(identity, existing ? preferProvider(existing, provider) : provider);
  }

  return Array.from(byIdentity.values());
}

export function toProviderInfo(
  options: Array<{
    id: string;
    name: string;
    presetName: string;
    baseUrl: string;
    keys: Array<{ id: string; label: string; maskedKey: string }>;
    models?: Array<{ id: string; displayName?: string }>;
    defaultModel?: string | null;
  }>,
): ProviderInfo[] {
  return options.map((p) => ({
    id: p.id,
    provider_type: p.presetName,
    display_name: p.name,
    api_base_url: p.baseUrl,
    default_model: p.defaultModel ?? null,
    has_active_key: p.keys.length > 0,
    models: (p.models ?? []).map((m) => ({ id: m.id, display_name: m.displayName ?? m.id })),
  }));
}

export function classifyProviderReadiness(p: ProviderInfo[]): ProviderReadiness {
  if (p.length === 0) return 'no_provider';
  return p.some((x) => x.models?.length && x.models.length > 0 && x.has_active_key)
    ? 'ready'
    : 'no_model';
}

export function selectAssistantModel(
  p: ProviderInfo[],
): { providerId: string; modelId: string } | null {
  for (const x of p) {
    if (!x.has_active_key) continue;
    if (!x.models?.length) continue;
    const m = x.default_model ?? x.models[0]?.id;
    if (m) return { providerId: x.id, modelId: m };
  }
  return null;
}

export function canTestDiscoveredModel(_m: unknown[]): boolean {
  return true;
}

export function connectionFingerprint(b: string, k: string): string {
  return b + '|' + k.slice(0, 8);
}

export function normalizeDiscoveredModels<T extends { id: string }>(m: T[]): T[] {
  return m.filter((x) => x.id.trim().length > 0);
}

export function selectDiscoveredModel<T extends { id: string }>(m: T[]): T | null {
  return m[0] ?? null;
}
