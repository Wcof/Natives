'use client';

/**
 * Phase 6 engine capability admin UI — MCP / Scheduler / Extensions / Skills
 * loaded exclusively via AssistantGateway → Protocol v2 (no streamChat / direct nativesAPI skill stores).
 */
import { useCallback, useEffect, useState } from 'react';
import { RefreshCw, Loader, Server, Clock, Puzzle, Sparkles } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { createDefaultGateway } from '@/lib/assistant-gateway';
import {
  loadCapabilityAdminDashboard,
  type ExtensionAdminSnapshot,
  type McpAdminSnapshot,
  type SchedulerAdminSnapshot,
  type SkillAdminSnapshot,
} from '@/lib/assistant-workspace/capability-admin';
import { SPACING } from '@/lib/design-tokens';

export default function EngineCapabilitiesPanel({ locale }: { locale: Locale }) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mcp, setMcp] = useState<McpAdminSnapshot | null>(null);
  const [scheduler, setScheduler] = useState<SchedulerAdminSnapshot | null>(null);
  const [extensions, setExtensions] = useState<ExtensionAdminSnapshot | null>(null);
  const [skills, setSkills] = useState<SkillAdminSnapshot | null>(null);

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
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const zh = locale === 'zh';

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
