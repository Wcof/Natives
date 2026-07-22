/**
 * Local persistence for non-execution UI state only:
 * layout (widths/collapse/tab) and per-conversation composer drafts.
 * Never persists Run/message/execution state or Provider keys.
 */
import type { AssistantViewState, ComposerDraft } from './state';

const VIEW_KEY = 'natives.assistant.view.v1';
const DRAFTS_KEY = 'natives.assistant.drafts.v1';

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

export function loadPersistedView(): Partial<AssistantViewState> | null {
  if (!canUseStorage()) return null;
  try {
    const raw = window.localStorage.getItem(VIEW_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<AssistantViewState>;
    return {
      leftCollapsed: parsed.leftCollapsed,
      rightCollapsed: parsed.rightCollapsed,
      leftWidth: typeof parsed.leftWidth === 'number' ? parsed.leftWidth : undefined,
      rightWidth: typeof parsed.rightWidth === 'number' ? parsed.rightWidth : undefined,
      inspectorTab: parsed.inspectorTab,
    };
  } catch {
    return null;
  }
}

export function savePersistedView(view: AssistantViewState): void {
  if (!canUseStorage()) return;
  try {
    window.localStorage.setItem(
      VIEW_KEY,
      JSON.stringify({
        leftCollapsed: view.leftCollapsed,
        rightCollapsed: view.rightCollapsed,
        leftWidth: view.leftWidth,
        rightWidth: view.rightWidth,
        inspectorTab: view.inspectorTab,
      }),
    );
  } catch {
    /* quota / private mode */
  }
}

export function loadPersistedDrafts(): Record<string, ComposerDraft> {
  if (!canUseStorage()) return {};
  try {
    const raw = window.localStorage.getItem(DRAFTS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, ComposerDraft>;
    if (!parsed || typeof parsed !== 'object') return {};
    return parsed;
  } catch {
    return {};
  }
}

export function savePersistedDrafts(drafts: Record<string, ComposerDraft>): void {
  if (!canUseStorage()) return;
  try {
    // Cap payload: keep last 40 conversations by updatedAt.
    // Never persist temp-* drafts — they would resurrect as ghost sessions after restart.
    const entries = Object.entries(drafts)
      .filter(([id]) => !id.startsWith('temp-'))
      .filter(([, d]) => d && (d.text?.trim() || (d.attachments?.length ?? 0) > 0))
      .sort((a, b) => String(b[1].updatedAt).localeCompare(String(a[1].updatedAt)))
      .slice(0, 40);
    window.localStorage.setItem(DRAFTS_KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    /* */
  }
}

export function clearPersistedDraft(conversationId: string): void {
  const all = loadPersistedDrafts();
  if (!(conversationId in all)) return;
  delete all[conversationId];
  savePersistedDrafts(all);
}


/** Preferred execution runtime for new runs (native | claude_cli | …). UI-only pin. */
const RUNTIME_PREF_KEY = 'natives.assistant.runtimePref.v1';

export function loadPreferredRuntimeId(): string | null {
  if (!canUseStorage()) return null;
  try {
    const v = window.localStorage.getItem(RUNTIME_PREF_KEY);
    return v && v.trim() ? v.trim() : null;
  } catch {
    return null;
  }
}

export function savePreferredRuntimeId(runtimeId: string | null | undefined): void {
  if (!canUseStorage()) return;
  try {
    if (!runtimeId || !runtimeId.trim()) {
      window.localStorage.removeItem(RUNTIME_PREF_KEY);
      return;
    }
    window.localStorage.setItem(RUNTIME_PREF_KEY, runtimeId.trim());
  } catch {
    /* quota / private mode */
  }
}
