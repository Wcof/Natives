'use client';

/**
 * Phase 6 engine capability admin UI — MCP / Scheduler / Extensions / Skills / Rate Limit
 * loaded exclusively via AssistantGateway → Protocol v2 (no streamChat / direct nativesAPI skill stores).
 */
import { useCallback, useEffect, useState } from 'react';
import { RefreshCw, Loader, Server, Clock, Puzzle, Sparkles, Zap } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { createDefaultGateway } from '@/lib/assistant-gateway';
import {
  loadCapabilityAdminDashboard,
  updateRateLimit,
  type EngineRateLimitSnapshot,
  type ExtensionAdminSnapshot,
  type McpAdminSnapshot,
  type SchedulerAdminSnapshot,
  type SkillAdminSnapshot,
} from '@/lib/assistant-workspace/capability-admin';
import { SPACING } from '@/lib/design-tokens';
import { useToast } from '@/components/ui/Toast';

export default function EngineCapabilitiesPanel({ locale }: { locale: Locale }) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mcp, setMcp] = useState<McpAdminSnapshot | null>(null);
  const [scheduler, setScheduler] = useState<SchedulerAdminSnapshot | null>(null);
  const [extensions, setExtensions] = useState<ExtensionAdminSnapshot | null>(null);
  const [skills, setSkills] = useState<SkillAdminSnapshot | null>(null);
  const [rateLimit, setRateLimit] = useState<EngineRateLimitSnapshot | null>(null);

  // ── Rate limit edit states ──
  const [editEnabled, setEditEnabled] = useState(true);
  const [editRpm, setEditRpm] = useState(10);
  const [saving, setSaving] = useState(false);

  const { toast } = useToast();

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const gateway = createDefaultGateway(false);
      await gateway.connect().catch(() => {
        // connect may fail offline; still attempt request (adapter throws clearly)
      });
      const dash = await loadCapabilityAdminDashboard(gateway);
      setMcp(dash.mcp);
      setScheduler(dash.scheduler);
      setExtensions(dash.extensions);
      setSkills(dash.skills);
      setRateLimit(dash.rateLimit);
      // Sync edit states from loaded data
      if (dash.rateLimit) {
        setEditEnabled(dash.rateLimit.settings.enabled);
        setEditRpm(dash.rateLimit.settings.requests_per_minute);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // ── Rate limit save handler ──
  const handleSaveRateLimit = useCallback(async () => {
    const rpm = editRpm;
    if (!Number.isInteger(rpm) || rpm < 1 || rpm > 600) {
      toast(t(locale, 'rateLimit.invalidRpm'), 'error');
      return;
    }
    setSaving(true);
    try {
      const gateway = createDefaultGateway(false);
      await gateway.connect().catch(() => {});
      const snapshot = await updateRateLimit(gateway, {
        enabled: editEnabled,
        requests_per_minute: rpm,
      });
      setRateLimit(snapshot);
      setEditEnabled(snapshot.settings.enabled);
      setEditRpm(snapshot.settings.requests_per_minute);
      toast(t(locale, 'rateLimit.saveSuccess'), 'success');
    } catch {
      toast(t(locale, 'rateLimit.saveFailed'), 'error');
    } finally {
      setSaving(false);
    }
  }, [editEnabled, editRpm, locale, toast]);

  const zh = locale === 'zh';
  const rpmValid = Number.isInteger(editRpm) && editRpm >= 1 && editRpm <= 600;
  const intervalSeconds = rpmValid ? (60000 / editRpm / 1000).toFixed(1) : '—';

  return (
    <div data-testid="engine-capabilities-panel">
      <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: SPACING.sm }}>
        <button
          type="button"
          className="btn inline-flex items-center gap-2 text-xs"
          onClick={() => void load()}
          disabled={loading}
        >
          {loading ? <Loader size={12} className="animate-spin" /> : <RefreshCw size={12} />}
          {t(locale, 'common.refresh')}
        </button>
      </div>

      {error ? (
        <div
          role="alert"
          className="rounded-lg border border-[var(--danger)] p-4 text-sm text-[var(--danger)]"
          data-testid="engine-capabilities-error"
        >
          {error}
          <div style={{ marginTop: 8 }}>
            <button type="button" className="btn btn-primary text-xs" onClick={() => void load()}>
              {t(locale, 'common.retry')}
            </button>
          </div>
        </div>
      ) : null}

      {/* ── Rate Limit Card ── */}
      <div className="settings-section-card" style={{ marginBottom: SPACING.md }}>
        <div className="settings-section-heading settings-section-heading-with-icon">
          <span className="settings-preference-icon"><Zap size={16} /></span>
          <div>
            <h4>{t(locale, 'rateLimit.title')}</h4>
          </div>
        </div>

        {loading && !rateLimit ? (
          <div style={{ padding: SPACING.md, color: 'var(--text-disabled)', fontSize: 13 }}>
            <Loader size={12} className="animate-spin" style={{ display: 'inline', marginRight: 6 }} />
            {t(locale, 'common.loading')}
          </div>
        ) : (
          <div style={{ padding: SPACING.md }}>
            {/* Enable toggle */}
            <label
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: SPACING.sm,
                cursor: 'pointer',
                marginBottom: SPACING.md,
                fontSize: 14,
              }}
            >
              <input
                type="checkbox"
                checked={editEnabled}
                onChange={(e) => setEditEnabled(e.target.checked)}
                style={{ accentColor: 'var(--primary)', width: 16, height: 16 }}
              />
              <span>{editEnabled ? t(locale, 'rateLimit.enabled') : t(locale, 'rateLimit.disabled')}</span>
            </label>

            {/* RPM input */}
            <div style={{ marginBottom: SPACING.md }}>
              <label
                style={{
                  display: 'block',
                  fontSize: 13,
                  fontWeight: 500,
                  marginBottom: SPACING.xs,
                  color: 'var(--text-body)',
                }}
              >
                {t(locale, 'rateLimit.rpm')}
              </label>
              <input
                type="number"
                min={1}
                max={600}
                step={1}
                value={editRpm}
                onChange={(e) => setEditRpm(Number(e.target.value))}
                disabled={!editEnabled}
                style={{
                  width: 120,
                  height: 36,
                  padding: '0 10px',
                  background: 'var(--surface)',
                  border: `1px solid ${rpmValid || !editEnabled ? 'var(--border)' : 'var(--danger)'}`,
                  borderRadius: 8,
                  color: 'var(--text)',
                  fontSize: 14,
                  outline: 'none',
                  opacity: editEnabled ? 1 : 0.5,
                }}
              />
              <span
                style={{
                  marginLeft: SPACING.sm,
                  fontSize: 12,
                  color: rpmValid || !editEnabled ? 'var(--text-disabled)' : 'var(--danger)',
                }}
              >
                {t(locale, 'rateLimit.rpmRange')}
              </span>
            </div>

            {/* Interval hint */}
            {editEnabled && rpmValid ? (
              <p
                style={{
                  fontSize: 12,
                  color: 'var(--text-disabled)',
                  marginBottom: SPACING.md,
                  marginTop: 0,
                }}
              >
                {t(locale, 'rateLimit.intervalHint', { seconds: intervalSeconds })}
              </p>
            ) : null}

            {/* Status display */}
            {rateLimit ? (
              <div
                style={{
                  display: 'flex',
                  gap: SPACING.lg,
                  marginBottom: SPACING.md,
                  fontSize: 13,
                  color: 'var(--text-secondary)',
                }}
              >
                <span>
                  {t(locale, 'rateLimit.queued')}:{' '}
                  <strong style={{ color: 'var(--text)' }}>{rateLimit.queued_requests}</strong>
                </span>
                <span>
                  {t(locale, 'rateLimit.cooling')}:{' '}
                  <strong style={{ color: 'var(--text)' }}>{rateLimit.cooling_routes}</strong>
                </span>
              </div>
            ) : null}

            {/* Save button */}
            <button
              type="button"
              className="btn btn-primary text-xs"
              disabled={saving || (!rpmValid && editEnabled)}
              onClick={() => void handleSaveRateLimit()}
            >
              {saving ? (
                <Loader size={12} className="animate-spin" style={{ display: 'inline', marginRight: 4 }} />
              ) : null}
              {t(locale, 'rateLimit.save')}
            </button>
          </div>
        )}
      </div>

      <Section
        icon={<Server size={16} />}
        title={zh ? 'MCP 服务器' : 'MCP servers'}
        count={mcp?.servers?.length ?? 0}
        empty={zh ? '暂无已注册 MCP 服务器' : 'No MCP servers registered'}
        loading={loading && !mcp}
      >
        <ul className="settings-plugin-list" style={{ listStyle: 'none', margin: 0, padding: 0 }}>
          {(mcp?.servers ?? []).map((s, i) => {
            const row = s as Record<string, unknown>;
            return (
              <li key={String(row.id ?? i)} className="settings-plugin-row">
                <strong>{String(row.id ?? row.name ?? 'server')}</strong>
                <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>
                  {String(row.transport ?? '')}
                  {row.trusted === true ? ' · trusted' : ''}
                </span>
              </li>
            );
          })}
        </ul>
        {(mcp?.tools?.length ?? 0) > 0 ? (
          <p style={{ fontSize: 12, color: 'var(--text-disabled)', marginTop: 8 }}>
            {zh ? '已发现工具' : 'Tools discovered'}: {mcp!.tools.length}
            {mcp!.namespaced?.length ? ` · namespaced ${mcp!.namespaced.length}` : ''}
          </p>
        ) : null}
      </Section>

      <Section
        icon={<Clock size={16} />}
        title={zh ? '调度任务' : 'Scheduler jobs'}
        count={scheduler?.jobs?.length ?? 0}
        empty={zh ? '暂无调度任务' : 'No scheduler jobs'}
        loading={loading && !scheduler}
      >
        <ul className="settings-plugin-list" style={{ listStyle: 'none', margin: 0, padding: 0 }}>
          {(scheduler?.jobs ?? []).map((j, i) => {
            const row = j as Record<string, unknown>;
            return (
              <li key={String(row.id ?? i)} className="settings-plugin-row">
                <strong>{String(row.id ?? row.name ?? `job-${i}`)}</strong>
                <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>
                  {String(row.kind ?? row.schedule ?? row.cron ?? '')}
                </span>
              </li>
            );
          })}
        </ul>
      </Section>

      <Section
        icon={<Puzzle size={16} />}
        title={zh ? '扩展' : 'Extensions'}
        count={extensions?.extensions?.length ?? 0}
        empty={zh ? '暂无扩展' : 'No extensions'}
        loading={loading && !extensions}
      >
        <ul className="settings-plugin-list" style={{ listStyle: 'none', margin: 0, padding: 0 }}>
          {(extensions?.extensions ?? []).map((e, i) => {
            const row = e as Record<string, unknown>;
            return (
              <li key={String(row.id ?? i)} className="settings-plugin-row">
                <strong>{String(row.id ?? row.name ?? `ext-${i}`)}</strong>
                <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>
                  {row.enabled === false ? (zh ? '已禁用' : 'disabled') : zh ? '已启用' : 'enabled'}
                </span>
              </li>
            );
          })}
        </ul>
      </Section>

      <Section
        icon={<Sparkles size={16} />}
        title={zh ? 'Skills' : 'Skills'}
        count={skills?.skills?.length ?? 0}
        empty={zh ? '暂无 Skills' : 'No skills discovered'}
        loading={loading && !skills}
      >
        <ul className="settings-plugin-list" style={{ listStyle: 'none', margin: 0, padding: 0 }}>
          {(skills?.skills ?? []).map((s, i) => {
            const row = s as Record<string, unknown>;
            return (
              <li key={String(row.name ?? row.id ?? i)} className="settings-plugin-row">
                <strong>{String(row.name ?? row.id ?? `skill-${i}`)}</strong>
                <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>
                  {String(row.source ?? row.path ?? '').slice(0, 80)}
                </span>
              </li>
            );
          })}
        </ul>
      </Section>
    </div>
  );
}

function Section({
  icon,
  title,
  count,
  empty,
  loading,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  count: number;
  empty: string;
  loading: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="settings-section-card" style={{ marginBottom: SPACING.md }}>
      <div className="settings-section-heading settings-section-heading-with-icon">
        <span className="settings-preference-icon">{icon}</span>
        <div>
          <h4>
            {title}{' '}
            <span style={{ color: 'var(--text-disabled)', fontWeight: 400, fontSize: 13 }}>
              ({count})
            </span>
          </h4>
        </div>
      </div>
      {loading ? (
        <div style={{ padding: SPACING.md, color: 'var(--text-disabled)', fontSize: 13 }}>
          <Loader size={12} className="animate-spin" style={{ display: 'inline', marginRight: 6 }} />
          …
        </div>
      ) : count === 0 ? (
        <div className="settings-plugin-empty" style={{ padding: SPACING.md }}>
          {empty}
        </div>
      ) : (
        children
      )}
    </div>
  );
}
