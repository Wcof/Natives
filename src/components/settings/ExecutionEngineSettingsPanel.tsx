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
import nativesAPI from '@/lib/tauri-adapter';

const adapter = nativesAPI;

interface RuntimeDescriptor {
  id: string;
  displayName: string;
  status: string;
  version?: string | null;
  authority: string;
  reasonCode: string;
  reason: string;
  capabilities: Record<string, string>;
  controllable: string[];
}

interface ResolvedDefaultRuntime {
  runtimeId: string;
  source: string;
  fallbackUsed: boolean;
  reasonCode: string;
  reason: string;
}

interface ExecutionEngineSnapshot {
  settings: {
    schemaVersion: number;
    revision: number;
    defaultRuntime: string;
    externalUnavailablePolicy: string;
    native: { maxSteps: number; disabledTools: string[] };
    claudeCli: { enabled: boolean };
    codexCli: { enabled: boolean };
    diagnostics: { performanceTelemetry: boolean };
  };
  runtimes: RuntimeDescriptor[];
  resolvedDefault: ResolvedDefaultRuntime;
  diagnosticsSummary: Record<string, unknown>;
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
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const snap = (await adapter.executionEngine.getSnapshot()) as unknown as ExecutionEngineSnapshot;
      setSnapshot(snap);
      setMaxStepsDraft(snap?.settings?.native?.maxSteps ?? 50);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }, []);

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
      await adapter.executionEngine.saveSettings(updated);
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
        await adapter.executionEngine.saveSettings(updated);
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
                {rt.status === 'blocked' || rt.status === 'degraded' || rt.status === 'disabled' ? (
                  <div style={{ fontSize: 12, color: 'var(--warning, #b45309)', marginTop: 4 }}>
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
                  disabled={busy}
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

      {/* 卡片 2 — 当前解析结果（backend 决定，UI 不猜） */}
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
              <ul style={{ margin: '4px 0 0', paddingLeft: 18 }}>
                {snapshot.settings.native.disabledTools.map((tool) => (
                  <li key={tool}>{tool}</li>
                ))}
              </ul>
            )}
            <div style={{ color: 'var(--text-secondary)', marginTop: 4 }}>
              {t(locale, 'executionEngine.disabledToolsHint')}
            </div>
          </div>
        ) : null}
      </Card>

      {/* 卡片 5 — 能力真相（backend descriptor） */}
      <Card title={t(locale, 'executionEngine.capabilitiesTitle')}>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
          <thead>
            <tr style={{ textAlign: 'left', borderBottom: '1px solid var(--border)' }}>
              <th style={{ padding: 6 }}>{t(locale, 'executionEngine.capability')}</th>
              <th style={{ padding: 6 }}>Native</th>
              <th style={{ padding: 6 }}>Claude CLI</th>
              <th style={{ padding: 6 }}>Codex</th>
            </tr>
          </thead>
          <tbody>
            {['streaming', 'tools', 'mcp', 'hooks', 'subagent', 'checkpoint_resume', 'side_effect_ledger', 'provider_routing'].map(
              (cap) => (
                <tr key={cap} style={{ borderBottom: '1px solid var(--border-soft, #eee)' }}>
                  <td style={{ padding: 6 }}>{cap}</td>
                  {['native', 'claude_cli', 'codex_cli'].map((rid) => {
                    const value =
                      snapshot.runtimes.find((r) => r.id === rid)?.capabilities[cap] ?? 'unknown';
                    return (
                      <td key={rid} style={{ padding: 6 }}>
                        {value}
                      </td>
                    );
                  })}
                </tr>
              ),
            )}
          </tbody>
        </table>
      </Card>

      {/* 卡片 6 — 高级诊断（折叠，只读） */}
      <Card title={t(locale, 'executionEngine.diagnosticsTitle')}>
        <pre
          style={{
            fontSize: 11,
            whiteSpace: 'pre-wrap',
            background: 'var(--bg-soft, #f6f6f6)',
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
