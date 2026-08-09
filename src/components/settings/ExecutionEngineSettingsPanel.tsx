'use client';

/**
 * ExecutionEngineSettingsPanel — 「运行设置」tab (A7).
 *
 * 所有状态来自 backend snapshot（A6 `execution_engine_get_snapshot`），
 * 不再以 localStorage 或前端 hard-code 为权威。
 *
 * 普通用户只控制产品策略：
 *   - 默认 Runtime
 *   - 外部 Runtime unavailable policy（仅显式 fallback_native）
 *   - Native maxSteps（Advanced）
 *   - 减法型 disabledTools（折叠，只做减法）
 *
 * 禁止暴露：UDS poll/flush、event durability、checkpoint/ledger 开关、
 * readonly fast path、doom-loop/circuit breaker 阈值。
 */

import { useEffect, useState, useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import { useToast } from '@/components/ui/Toast';
import { executionEngine } from '@/lib/tauri/execution-engine';
import type { ExecutionEngineSnapshot, RuntimeDescriptor } from '@/lib/tauri/types';
import { classifyError } from '@/lib/error-classifier';
import {
  clearPreferredRuntimeId,
  loadPreferredRuntimeId,
} from '@/lib/assistant-workspace/persistence';

/// Known Native tool surface (subtract-only candidates). Free-form entries
/// are allowed too — this list is a convenience, not an authority.
const KNOWN_TOOLS = ['read_file', 'list_dir', 'write_file', 'write_module', 'run_terminal', 'lint_module'];

/** SETTINGS-001: only a currently-`ready` runtime is a valid default choice.
 *  blocked / degraded / disabled / not_installed radios are disabled — the
 *  Host save gate independently rejects them (double-layer validation). */
export function isSelectable(runtime: RuntimeDescriptor): boolean {
  return runtime.status === 'ready';
}

function statusLabel(locale: Locale, status: string): string {
  const map: Record<string, string> = {
    ready: t(locale, 'executionEngine.ready'),
    degraded: t(locale, 'executionEngine.degraded'),
    blocked: t(locale, 'executionEngine.blocked'),
    disabled: t(locale, 'executionEngine.disabled'),
    not_installed: t(locale, 'executionEngine.notInstalled'),
  };
  return map[status] ?? status;
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="settings-section-card" style={{ marginTop: 16 }}>
      <div className="settings-section-heading">
        <h4>{title}</h4>
      </div>
      <div style={{ display: 'grid', gap: 10, fontSize: 13 }}>{children}</div>
    </div>
  );
}

export default function ExecutionEngineSettingsPanel({ locale }: { locale: Locale }) {
  const { toast } = useToast();
  const [snapshot, setSnapshot] = useState<ExecutionEngineSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [maxStepsDraft, setMaxStepsDraft] = useState<number>(50);
  const [disabledToolDraft, setDisabledToolDraft] = useState<string>('');
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      // MIG-001：仅首次（本地仍残留旧 key 时）把 legacy localStorage runtime pref
      // 作为一次性迁移种子传给后端，后端做 one-way 迁移到 Settings V2 defaultRuntime
      // （revision CAS 保证只迁一次，绝不覆盖已权威的 V2）。迁移成功/无需迁移后
      // 立即清除旧 key — 此后 getter 不再读取旧值，新 Run 默认完全由 V2 决定。
      const snap = await executionEngine.getSnapshot(loadPreferredRuntimeId());
      clearPreferredRuntimeId();
      setSnapshot(snap);
      setMaxStepsDraft(snap.settings.native.maxSteps ?? 50);
    } catch (err) {
      const classified = classifyError(err, { locale });
      setError(classified.userMessage);
    } finally {
      setBusy(false);
    }
  }, [locale]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveMaxSteps = useCallback(async () => {
    if (!snapshot) return;
    setBusy(true);
    try {
      const updated = {
        ...snapshot.settings,
        native: { ...snapshot.settings.native, maxSteps: Math.min(200, Math.max(10, maxStepsDraft)) },
      };
      await executionEngine.saveSettings(updated);
      toast(t(locale, 'executionEngine.saved'), 'success');
      await refresh();
    } catch (err) {
      toast(String(err), 'error');
    } finally {
      setBusy(false);
    }
  }, [snapshot, maxStepsDraft, locale, toast, refresh]);

  const saveDefaultRuntime = useCallback(
    async (runtimeId: string, fallback?: string) => {
      if (!snapshot) return;
      setBusy(true);
      try {
        const updated = {
          ...snapshot.settings,
          defaultRuntime: runtimeId,
          ...(fallback ? { externalUnavailablePolicy: fallback } : {}),
        };
        await executionEngine.saveSettings(updated);
        toast(t(locale, 'executionEngine.saved'), 'success');
        await refresh();
      } catch (err) {
        toast(String(err), 'error');
      } finally {
        setBusy(false);
      }
    },
    [snapshot, locale, toast, refresh],
  );

  // 减法型 disabledTools：从当前快照的 disabledTools 出发，只允许用户“加”或
  // “减”自己保存的禁用条目（减掉的条目回到 capability 表面，永远不会把
  // capability 表面之外的工具加回来）。CAS revision 冲突由后端拒绝。
  const saveDisabledTools = useCallback(
    async (nextDisabled: string[]) => {
      if (!snapshot) return;
      setBusy(true);
      try {
        const normalized = Array.from(new Set(nextDisabled.map((s) => s.trim()).filter(Boolean))).sort();
        const updated = {
          ...snapshot.settings,
          native: { ...snapshot.settings.native, disabledTools: normalized },
        };
        await executionEngine.saveSettings(updated);
        toast(t(locale, 'executionEngine.saved'), 'success');
        await refresh();
      } catch (err) {
        toast(String(err), 'error');
      } finally {
        setBusy(false);
      }
    },
    [snapshot, locale, toast, refresh],
  );

  const addDisabledTool = useCallback(() => {
    if (!snapshot) return;
    const tool = disabledToolDraft.trim();
    if (!tool) return;
    void saveDisabledTools([...snapshot.settings.native.disabledTools, tool]);
    setDisabledToolDraft('');
  }, [snapshot, disabledToolDraft, saveDisabledTools]);

  const removeDisabledTool = useCallback(
    (tool: string) => {
      if (!snapshot) return;
      void saveDisabledTools(
        snapshot.settings.native.disabledTools.filter((existing) => existing !== tool),
      );
    },
    [snapshot, saveDisabledTools],
  );

  if (error && !snapshot) {
    return (
      <div className="settings-section-card" style={{ marginTop: 16, color: 'var(--danger)' }}>
        <div className="settings-section-heading">
          <h4>{t(locale, 'executionEngine.title')}</h4>
          <p>{error}</p>
        </div>
        <button type="button" className="btn btn-secondary" onClick={() => void refresh()}>
          {t(locale, 'common.retry')}
        </button>
      </div>
    );
  }
  if (!snapshot) {
    return <div style={{ padding: 24 }}>{t(locale, 'common.loading')}</div>;
  }

  const defaultRt = snapshot.runtimes.find((r) => r.id === snapshot.settings.defaultRuntime);
  const resolved = snapshot.resolvedDefault;
  // SETTINGS-002: capability rows come from the real daemon-advertised matrix
  // projected into each descriptor — never a hardcoded feature list.
  const capabilityKeys = Array.from(
    new Set(snapshot.runtimes.flatMap((rt) => Object.keys(rt.capabilities))),
  ).sort();

  return (
    <div>
      {/* 卡片 1 — 默认 Runtime */}
      <Card title={t(locale, 'executionEngine.defaultRuntimeTitle')}>
        <p style={{ color: 'var(--text-secondary)', margin: '0 0 8px' }}>
          {t(locale, 'executionEngine.defaultRuntimeDesc')}
        </p>
        <div style={{ display: 'grid', gap: 8 }}>
          {snapshot.runtimes.map((rt) => (
            <div
              key={rt.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 12,
                padding: '10px 12px',
                borderRadius: 8,
                border: `1px solid ${
                  snapshot.settings.defaultRuntime === rt.id ? 'var(--accent)' : 'var(--border)'
                }`,
                background: snapshot.settings.defaultRuntime === rt.id ? 'var(--accent-soft, transparent)' : 'transparent',
              }}
            >
              <div style={{ flex: 1 }}>
                <strong>
                  {rt.displayName}
                  {rt.id === 'native' ? ` — ${t(locale, 'executionEngine.recommended')}` : ''}
                </strong>
                <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
                  {t(locale, 'executionEngine.authority')}: {rt.authority} ·{' '}
                  {t(locale, 'executionEngine.status')}: {statusLabel(locale, rt.status)}
                  {rt.version ? ` · ${rt.version}` : ''}
                </div>
                {!isSelectable(rt) ? (
                  <div style={{ fontSize: 12, color: 'var(--warning)', marginTop: 4 }}>
                    {rt.reason}
                  </div>
                ) : null}
              </div>
              <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                <input
                  type="radio"
                  name="defaultRuntime"
                  checked={snapshot.settings.defaultRuntime === rt.id}
                  onChange={() => void saveDefaultRuntime(rt.id)}
                  disabled={busy || !isSelectable(rt)}
                  title={isSelectable(rt) ? undefined : rt.reason}
                />
                {rt.id === 'claude_cli' && snapshot.settings.defaultRuntime === rt.id ? (
                  <select
                    value={snapshot.settings.externalUnavailablePolicy}
                    onChange={(e) => void saveDefaultRuntime(rt.id, e.target.value)}
                    style={{ fontSize: 12, padding: '4px 6px', borderRadius: 6, border: '1px solid var(--border)' }}
                  >
                    <option value="fail">{t(locale, 'executionEngine.policyFail')}</option>
                    <option value="fallback_native">{t(locale, 'executionEngine.policyFallbackNative')}</option>
                  </select>
                ) : null}
              </div>
            </div>
          ))}
        </div>
      </Card>

      {/* 卡片 2 — 当前解析结果（backend 决定，UI 不猜）+ 实际协商的传输状态 */}
      <Card title={t(locale, 'executionEngine.resolvedTitle')}>
        <div>
          <strong>{t(locale, 'executionEngine.effectiveRuntime')}</strong>: {resolved.runtimeId}
        </div>
        <div>
          <strong>{t(locale, 'executionEngine.configSource')}</strong>: {resolved.source}
        </div>
        <div>
          <strong>{t(locale, 'executionEngine.fallbackUsed')}</strong>:{' '}
          {resolved.fallbackUsed ? t(locale, 'common.yes') : t(locale, 'common.no')}
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{resolved.reason}</div>
        {/* Active negotiated transport — backend truth, never a fake config flag. */}
        <div style={{ marginTop: 8, fontSize: 12, borderTop: '1px solid var(--border-soft)', paddingTop: 8 }}>
          <div>
            <strong>{locale.startsWith('zh') ? '实际协商传输' : 'Negotiated transport'}</strong>:{' '}
            {String(snapshot.diagnosticsSummary.streamTransport ?? 'unknown')}
          </div>
          <div>
            <strong>{locale.startsWith('zh') ? '运行权威' : 'Run authority'}</strong>:{' '}
            {String(snapshot.diagnosticsSummary.authorityMode ?? 'unknown')}
          </div>
          <div>
            <strong>{locale.startsWith('zh') ? 'Daemon 可达' : 'Daemon ready'}</strong>:{' '}
            {snapshot.diagnosticsSummary.daemonReady ? t(locale, 'common.yes') : t(locale, 'common.no')}
          </div>
        </div>
      </Card>

      {/* 卡片 3 — Native Engine（普通 UI 只留 maxSteps + 减法覆盖） */}
      <Card title={t(locale, 'executionEngine.nativeEngineTitle')}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <label htmlFor="native-max-steps">{t(locale, 'executionEngine.maxSteps')}</label>
          <input
            id="native-max-steps"
            type="number"
            min={10}
            max={200}
            value={maxStepsDraft}
            onChange={(e) => setMaxStepsDraft(Number(e.target.value))}
            style={{ width: 90, padding: '6px 8px', borderRadius: 6, border: '1px solid var(--border)' }}
          />
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() => void saveMaxSteps()}
            disabled={busy}
          >
            {t(locale, 'common.save')}
          </button>
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 4 }}>
          {t(locale, 'executionEngine.maxStepsHint')}
        </div>
        <button
          type="button"
          className="btn btn-secondary"
          style={{ marginTop: 8 }}
          onClick={() => setAdvancedOpen((v) => !v)}
        >
          {t(locale, 'executionEngine.advanced')}
        </button>
        {advancedOpen ? (
          <div style={{ marginTop: 8, fontSize: 12 }}>
            <div>
              <strong>{t(locale, 'executionEngine.disabledToolsTitle')}</strong>
            </div>
            {snapshot.settings.native.disabledTools.length === 0 ? (
              <div style={{ color: 'var(--text-secondary)' }}>
                {t(locale, 'executionEngine.noDisabledTools')}
              </div>
            ) : (
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, margin: '6px 0' }}>
                {snapshot.settings.native.disabledTools.map((tool) => (
                  <span
                    key={tool}
                    style={{
                      display: 'inline-flex',
                      alignItems: 'center',
                      gap: 6,
                      padding: '3px 8px',
                      borderRadius: 999,
                      border: '1px solid var(--border)',
                      background: 'var(--bg-soft)',
                    }}
                  >
                    {tool}
                    <button
                      type="button"
                      aria-label={`remove ${tool}`}
                      onClick={() => removeDisabledTool(tool)}
                      disabled={busy}
                      style={{
                        border: 'none',
                        background: 'transparent',
                        cursor: 'pointer',
                        color: 'var(--text-secondary)',
                        fontSize: 13,
                        lineHeight: 1,
                        padding: 0,
                      }}
                    >
                      ×
                    </button>
                  </span>
                ))}
              </div>
            )}
            {/* 减法编辑器：加入的是“禁用”，永远不能把 capability 表面之外的
                工具加回来；saveSettings 的 revision CAS 拒绝跨窗口覆盖。 */}
            <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
              <input
                type="text"
                value={disabledToolDraft}
                onChange={(e) => setDisabledToolDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    addDisabledTool();
                  }
                }}
                placeholder={locale.startsWith('zh') ? '工具名（减法禁用）' : 'tool name (subtract)'}
                style={{ flex: 1, padding: '5px 8px', borderRadius: 6, border: '1px solid var(--border)', fontSize: 12 }}
              />
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => addDisabledTool()}
                disabled={busy}
                style={{ fontSize: 12 }}
              >
                {t(locale, 'common.save')}
              </button>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 6 }}>
              {KNOWN_TOOLS.filter(
                (known) => !snapshot.settings.native.disabledTools.includes(known),
              ).map((known) => (
                <button
                  key={known}
                  type="button"
                  onClick={() => void saveDisabledTools([...snapshot.settings.native.disabledTools, known])}
                  disabled={busy}
                  style={{
                    fontSize: 11,
                    padding: '2px 8px',
                    borderRadius: 999,
                    border: '1px dashed var(--border)',
                    background: 'transparent',
                    cursor: busy ? 'default' : 'pointer',
                    color: 'var(--text-secondary)',
                  }}
                >
                  + {known}
                </button>
              ))}
            </div>
            <div style={{ color: 'var(--text-secondary)', marginTop: 6 }}>
              {t(locale, 'executionEngine.disabledToolsHint')}
            </div>
          </div>
        ) : null}
      </Card>

      {/* 卡片 5 — 能力真相（真实 daemon 探测投影，非 Host 静态表） */}
      <Card title={t(locale, 'executionEngine.capabilitiesTitle')}>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
          <thead>
            <tr style={{ textAlign: 'left', borderBottom: '1px solid var(--border)' }}>
              <th style={{ padding: 6 }}>{t(locale, 'executionEngine.capability')}</th>
              {snapshot.runtimes.map((rt) => (
                <th key={rt.id} style={{ padding: 6 }}>
                  {rt.displayName}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {capabilityKeys.map((cap) => (
              <tr key={cap} style={{ borderBottom: '1px solid var(--border-soft)' }}>
                <td style={{ padding: 6 }}>{cap}</td>
                {snapshot.runtimes.map((rt) => {
                  const value = rt.capabilities[cap] ?? 'unknown';
                  return (
                    <td key={rt.id} style={{ padding: 6 }}>
                      {value}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      {/* 卡片 6 — 高级诊断（折叠，只读） */}
      <Card title={t(locale, 'executionEngine.diagnosticsTitle')}>
        <pre
          style={{
            fontSize: 11,
            whiteSpace: 'pre-wrap',
            background: 'var(--bg-soft)',
            padding: 8,
            borderRadius: 6,
          }}
        >
          {JSON.stringify(snapshot.diagnosticsSummary, null, 2)}
        </pre>
      </Card>

      <div style={{ marginTop: 12 }}>
        <button type="button" className="btn btn-secondary" onClick={() => void refresh()} disabled={busy}>
          {t(locale, 'executionEngine.refresh')}
        </button>
      </div>
    </div>
  );
}
