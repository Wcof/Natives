'use client';

import { useCallback, useEffect, useState, useMemo } from 'react';
import { Square, HardDrive, Database, FileText, Zap, Eye, Archive, AlertTriangle, RefreshCw } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { useRecentModules } from '@/lib/recent-modules';
import { useRecentFiles } from '@/lib/recent-files-client';
import { useAsyncData } from '@/hooks/useAsyncData';
import { ProgressBar, TokenChip } from '@/components/ui/ProgressBar';
import type { SkillInfo } from '@/types/agent';

interface ModuleInfo {
  id: string;
  name: string;
  icon?: string;
  version?: string;
  enabled?: number;
}

interface NotificationItem {
  id: number;
  moduleId: string;
  title: string;
  body?: string;
  level: string;
  read: number;
  createdAt: string;
}

interface SystemStatus {
  dbOk: boolean;
  moduleCount: number;
  enabledCount: number;
  version: string;
  diskUsage: string;
  unreadNotifications: number;
}

export default function DashboardPage() {
  const [locale, setLocale] = useState<Locale>('zh');
  const [allModules, setAllModules] = useState<ModuleInfo[]>([]);
  const [recentModules, setRecentModules] = useState<ModuleInfo[]>([]);
  const [notifications, setNotifications] = useState<NotificationItem[]>([]);
  const [status, setStatus] = useState<SystemStatus>({
    dbOk: false,
    moduleCount: 0,
    enabledCount: 0,
    version: '...',
    diskUsage: '...',
    unreadNotifications: 0,
  });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [retryCount, setRetryCount] = useState(0);

  // LRU list of recently-opened module ids
  const { ids: recentIds } = useRecentModules(8);

  // LRU list of recently-opened files
  const { paths: recentFilePaths, loading: recentFilesLoading } = useRecentFiles(8);

  // Skill stats
  const { data: skills, loading: skillsLoading } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (!api?.agent?.scanSkills) return [] as SkillInfo[];
    return (await api.agent.scanSkills()) as SkillInfo[];
  }, []);

  const skillStats = useMemo(() => {
    if (!Array.isArray(skills)) return { total: 0, active: 0, dust: 0, issues: 0, descChars: 0, descLimit: 15000 };
    const total = skills.length;
    const active = skills.filter((s) => s.triggerCount > 0).length;
    const dust = skills.filter((s) => s.triggerCount === 0).length;
    const issues = skills.filter((s) => !s.health?.ok).length;
    const descChars = skills.reduce((sum, s) => sum + (s.description?.length || 0), 0);
    return { total, active, dust, issues, descChars, descLimit: 15000 };
  }, [skills]);

  // Usage data
  const { data: usageData, loading: usageLoading, reload: fetchUsage } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (!api?.usage?.refresh) return null;
    return await api.usage.refresh();
  }, []);

  useEffect(() => {
    async function loadData() {
      setLoading(true);
      try {
        const api = window.nativesAPI;
        if (!api) return;

        const savedLocale = await api.getLocale().catch(() => null);
        if (savedLocale === 'en') setLocale('en');

        // App version from package.json
        let version = '...';
        try {
          if (api.app?.version) version = await api.app.version();
        } catch { /* keep '...' */ }

        // Load modules
        const modules = await api.module.scan();
        let modList: ModuleInfo[] = [];
        if (Array.isArray(modules)) {
          const scanned = modules as Array<{ manifest?: { id: string; name: string; icon?: string; version?: string }; enabled?: number }>;
          modList = scanned
            .filter((m) => m.manifest)
            .map((m) => ({
              id: m.manifest!.id,
              name: m.manifest!.name,
              icon: m.manifest!.icon,
              version: m.manifest!.version,
              enabled: m.enabled,
            }));
          setAllModules(modList);
        }

        // DB health check
        let dbOk = false;
        try {
          await api.db.get('__health_check__');
          dbOk = true;
        } catch {
          dbOk = false;
        }

        // Disk usage
        let diskUsageStr = '—';
        try {
          if (api.disk?.usage) {
            const usage = await api.disk.usage('~/.natives');
            if (Array.isArray(usage) && usage.length > 0) {
              const totalBytes = usage.reduce((sum: number, item: { size?: number }) => sum + (item.size || 0), 0);
              diskUsageStr = formatBytes(totalBytes);
            }
          }
        } catch { /* ignore */ }

        // Recent notifications + unread count
        let unread = 0;
        let recentNotifs: NotificationItem[] = [];
        try {
          if (api.notification?.list) {
            const all = await api.notification.list(false);
            if (Array.isArray(all)) {
              recentNotifs = (all as NotificationItem[]).slice(0, 5);
              setNotifications(recentNotifs);
            }
            const unreadList = await api.notification.list(true);
            unread = Array.isArray(unreadList) ? unreadList.length : 0;
          }
        } catch { /* ignore */ }

        setStatus({
          dbOk,
          moduleCount: modList.length,
          enabledCount: modList.filter((m) => m.enabled).length,
          version,
          diskUsage: diskUsageStr,
          unreadNotifications: unread,
        });
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load dashboard data');
      } finally {
        setLoading(false);
      }
    }
    loadData();
  }, [retryCount]);

  // Derive "recently used" modules from LRU id list
  useEffect(() => {
    if (allModules.length === 0) {
      setRecentModules([]);
      return;
    }
    const byId = new Map(allModules.map((m) => [m.id, m]));
    const ordered = recentIds
      .map((id) => byId.get(id))
      .filter((m): m is ModuleInfo => Boolean(m));
    setRecentModules(ordered.length > 0 ? ordered : allModules.slice(0, 5));
  }, [recentIds, allModules]);

  const handleViewAllSkills = useCallback(() => {
    window.dispatchEvent(new CustomEvent('navigate', { detail: 'ai' }));
  }, []);

  const sectionTitleStyle: React.CSSProperties = {
    fontSize: 11, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)',
    textTransform: 'uppercase', letterSpacing: 1,
    marginBottom: 12,
  };

  const cardStyle: React.CSSProperties = {
    padding: '16px 20px',
    border: '1px solid var(--border,#262920)',
    borderRadius: 8,
  };

  return (
    <div style={{ height: '100%', overflow: 'auto', padding: '24px 28px' }}>
      {/* Header */}
      <div style={{ marginBottom: 28 }}>
        <h1 style={{ fontSize: 24, fontWeight: 600, color: 'var(--text)', margin: 0 }}>
          {t(locale, 'dashboard.title')}
        </h1>
        <p style={{ color: 'var(--text-faint)', fontSize: 13, marginTop: 4 }}>
          {t(locale, 'tagline')}
        </p>
      </div>

      {/* Error banner with retry */}
      {error && (
        <div style={{
          marginBottom: 16, padding: '10px 16px', borderRadius: 6,
          background: '#d9534f15', border: '1px solid #d9534f44',
          display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12,
        }}>
          <span style={{ fontSize: 12, color: 'var(--danger)' }}>⚠ {error}</span>
          <button
            onClick={() => { setError(null); setRetryCount(c => c + 1); }}
            style={{
              padding: '4px 12px', borderRadius: 4, border: '1px solid #d9534f44',
              background: 'transparent', color: 'var(--danger)', fontSize: 11, cursor: 'pointer',
            }}
          >
            Retry
          </button>
        </div>
      )}

      {/* System Status Grid */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(170px, 1fr))', gap: 12, marginBottom: 24 }}>
        <StatusCard
          icon={<Database size={16} />}
          label={t(locale, 'dashboard.statusDatabase')}
          value={loading ? t(locale, 'common.loading') : status.dbOk ? t(locale, 'dashboard.statusConnected') : t(locale, 'dashboard.statusError')}
          valueColor={loading ? 'var(--text-faint)' : status.dbOk ? 'var(--accent,#cdf24b)' : 'var(--danger)'}
        />
        <StatusCard
          icon={<Square size={16} />}
          label={t(locale, 'dashboard.installedModules')}
          value={loading ? '...' : `${status.enabledCount}/${status.moduleCount}`}
          sublabel={loading ? undefined : t(locale, 'dashboard.enabledModules')}
        />
        <StatusCard
          icon={<HardDrive size={16} />}
          label={t(locale, 'dashboard.statusDataUsage')}
          value={loading ? '...' : status.diskUsage}
        />
        <StatusCard
          icon={<RefreshCw size={16} />}
          label={t(locale, 'settings.version')}
          value={status.version}
        />
      </div>

      {/* Skill Insights */}
      <div style={{ ...cardStyle, marginBottom: 24 }}>
        <div style={{ ...sectionTitleStyle, display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <span>{t(locale, 'dashboard.skillInsights')}</span>
          <button
            className="btn btn-ghost"
            onClick={handleViewAllSkills}
            style={{ fontSize: 10, padding: '2px 8px', textTransform: 'none', letterSpacing: 0 }}
          >
            {t(locale, 'dashboard.viewAllSkills')} →
          </button>
        </div>
        {skillsLoading ? (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>{t(locale, 'common.loading')}</div>
        ) : skillStats.total === 0 ? (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>{t(locale, 'aiWorkbench.noSkills')}</div>
        ) : (
          <>
            <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
              <StatCard
                label={t(locale, 'aiWorkbench.skills.total')}
                value={skillStats.total}
                icon={<Zap size={14} />}
                color="var(--accent,#cdf24b)"
              />
              <StatCard
                label={t(locale, 'aiWorkbench.skills.active')}
                value={skillStats.active}
                icon={<Eye size={14} />}
                color="var(--info)"
              />
              <StatCard
                label={t(locale, 'aiWorkbench.skills.dust')}
                value={skillStats.dust}
                icon={<Archive size={14} />}
                color="#9CA3AF"
                sub={t(locale, 'aiWorkbench.skills.dustLabel')}
              />
              <StatCard
                label={t(locale, 'aiWorkbench.skills.issues')}
                value={skillStats.issues}
                icon={<AlertTriangle size={14} />}
                color={skillStats.issues > 0 ? 'var(--danger)' : 'var(--info)'}
              />
            </div>
            {/* Budget bar */}
            <BudgetBar used={skillStats.descChars} limit={skillStats.descLimit} locale={locale} />
          </>
        )}
      </div>

      {/* Usage Analysis */}
      <div style={{ ...cardStyle, marginBottom: 24 }}>
        <div style={{ ...sectionTitleStyle, display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <span>{t(locale, 'dashboard.usageAnalysis')}</span>
          <button
            className="btn btn-ghost"
            onClick={fetchUsage}
            disabled={usageLoading}
            style={{ fontSize: 10, padding: '2px 8px', textTransform: 'none', letterSpacing: 0 }}
          >
            {usageLoading ? '...' : t(locale, 'aiWorkbench.refresh')}
          </button>
        </div>
        {usageLoading ? (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>{t(locale, 'common.loading')}</div>
        ) : usageData ? (
          <div>
            {/* Claude Code */}
            <div style={{ marginBottom: 16 }}>
              <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)', marginBottom: 6 }}>Claude Code</div>
              <ProgressBar
                label={t(locale, 'aiWorkbench.fiveHourWindow')}
                used={(usageData as any)?.claude?.fiveHourWindow?.used ?? 0}
                limit={(usageData as any)?.claude?.fiveHourWindow?.limit ?? 50000}
                color="var(--accent,#cdf24b)"
              />
              <ProgressBar
                label={t(locale, 'aiWorkbench.weeklyQuota')}
                used={(usageData as any)?.claude?.weeklyQuota?.used ?? 0}
                limit={(usageData as any)?.claude?.weeklyQuota?.limit ?? 500000}
                color="#4bcdf2"
              />
              <div style={{ fontSize: 10, color: 'var(--text-faint,#62655a)', marginTop: -6, marginBottom: 8 }}>
                {t(locale, 'aiWorkbench.nextReset')}: {formatNextWednesday()}
              </div>
              <div style={{ display: 'flex', gap: 4, marginTop: 8 }}>
                <TokenChip value={(usageData as any)?.claude?.localTokens?.last5h ?? 0} label={t(locale, 'aiWorkbench.localTokens') + ' 5h'} />
                <TokenChip value={(usageData as any)?.claude?.localTokens?.today ?? 0} label={t(locale, 'aiWorkbench.today')} />
                <TokenChip value={(usageData as any)?.claude?.localTokens?.thisWeek ?? 0} label={t(locale, 'aiWorkbench.thisWeek')} />
              </div>
            </div>
            {/* Codex */}
            <div>
              <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)', marginBottom: 6 }}>Codex</div>
              <ProgressBar
                label={t(locale, 'aiWorkbench.fiveHourWindow')}
                used={(usageData as any)?.codex?.fiveHourWindow?.used ?? 0}
                limit={(usageData as any)?.codex?.fiveHourWindow?.limit ?? 30000}
                color="#DEA584"
              />
              <div style={{ fontSize: 11, color: 'var(--text-dim)', marginTop: 4 }}>
                {t(locale, 'aiWorkbench.plan')}: Free
              </div>
            </div>
          </div>
        ) : (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>
            {t(locale, 'common.loading')}
          </div>
        )}
      </div>

      {/* Recent Modules */}
      <div style={{ ...cardStyle, marginBottom: 24 }}>
        <div style={{ ...sectionTitleStyle, display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <span>{t(locale, 'dashboard.lastUsed')}</span>
          {recentIds.length > 0 && (
            <span style={{ fontSize: 10, color: 'var(--text-faint,#62655a)', textTransform: 'none', letterSpacing: 0 }}>
              {t(locale, 'dashboard.recentlyOpened')}
            </span>
          )}
        </div>
        {loading ? (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>{t(locale, 'common.loading')}</div>
        ) : recentModules.length === 0 ? (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>
            {t(locale, 'dashboard.noModules')}
          </div>
        ) : (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 8 }}>
            {recentModules.map((mod) => (
              <button
                key={mod.id}
                className="btn"
                onClick={() => window.dispatchEvent(new CustomEvent('navigate', { detail: `module:${mod.id}` }))}
                style={{
                  display: 'flex', alignItems: 'center', gap: 10,
                  padding: '10px 12px', fontSize: 13, justifyContent: 'flex-start',
                  width: '100%',
                }}
              >
                <span style={{ width: 20, height: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
                  {mod.icon ? (
                    <img src={mod.icon} alt="" style={{ width: 18, height: 18 }} />
                  ) : (
                    <Square size={16} style={{ color: 'var(--text-faint)' }} />
                  )}
                </span>
                <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{mod.name}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Recent Files */}
      <div style={{ ...cardStyle, marginBottom: 24 }}>
        <div style={sectionTitleStyle}>{t(locale, 'dashboard.recentFiles')}</div>
        {recentFilesLoading ? (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>{t(locale, 'common.loading')}</div>
        ) : recentFilePaths.length === 0 ? (
          <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>
            {t(locale, 'dashboard.recentFilesEmpty')}
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {recentFilePaths.map((filePath) => {
              const name = filePath.split('/').pop() || filePath;
              const dir = filePath.substring(0, filePath.lastIndexOf('/')) || '/';
              return (
                <button
                  key={filePath}
                  className="btn btn-ghost"
                  onClick={() => window.dispatchEvent(new CustomEvent('navigate', { detail: `files?path=${encodeURIComponent(dir)}&select=${encodeURIComponent(filePath)}` }))}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 8,
                    padding: '6px 10px', fontSize: 12, justifyContent: 'flex-start',
                    width: '100%', textAlign: 'left' as const,
                  }}
                >
                  <span style={{ flexShrink: 0, display: 'inline-flex', color: 'var(--text-faint)' }}>
                    <FileText size={14} />
                  </span>
                  <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontFamily: 'var(--font-mono)' }}>
                    {name}
                  </span>
                  <span style={{ color: 'var(--text-faint,#62655a)', fontSize: 10, marginLeft: 'auto', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 200 }}>
                    {dir}
                  </span>
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* Recent Activity */}
      {notifications.length > 0 && (
        <div style={cardStyle}>
          <div style={{ ...sectionTitleStyle, display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <span>{t(locale, 'dashboard.recentActivity')}</span>
            {status.unreadNotifications > 0 && (
              <span style={{
                fontSize: 10, padding: '1px 6px', borderRadius: 9,
                background: 'var(--danger-soft)', color: 'var(--danger)',
                fontWeight: 600, textTransform: 'none', letterSpacing: 0,
              }}>
                {status.unreadNotifications} {t(locale, 'dashboard.unreadNotifications')}
              </span>
            )}
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {notifications.map((notif) => (
              <div key={notif.id} style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '9px 12px',
                background: 'var(--bg-3,#1c1e17)', border: '1px solid var(--border,#262920)',
                borderRadius: 6, fontSize: 12,
              }}>
                <span style={{ flexShrink: 0, display: 'inline-flex' }}>
                  {notif.level === 'error' ? <span style={{ color: 'var(--danger)', fontSize: 10 }}>●</span> : notif.level === 'warning' ? <span style={{ color: 'var(--warning)', fontSize: 10 }}>●</span> : <span style={{ color: 'var(--info)', fontSize: 10 }}>●</span>}
                </span>
                <span style={{ color: 'var(--text)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {notif.title}
                </span>
                <span style={{ color: 'var(--text-faint,#62655a)', fontSize: 10, flexShrink: 0, fontFamily: 'var(--font-mono)' }}>
                  {formatRelativeTime(notif.createdAt)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// ── Sub-components ──

function StatusCard({ icon, label, value, valueColor, sublabel }: {
  icon: React.ReactNode;
  label: string;
  value: string;
  valueColor?: string;
  sublabel?: string;
}) {
  return (
    <div style={{
      padding: '14px 16px',
      background: 'var(--bg-2,#131410)',
      border: '1px solid var(--border,#262920)',
      borderRadius: 8,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, color: 'var(--text-dim)', fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.5 }}>
        {icon}
        <span>{label}</span>
      </div>
      <div style={{ fontSize: 20, fontWeight: 600, color: valueColor || 'var(--text)', fontFamily: 'var(--font-mono)' }}>
        {value}
      </div>
      {sublabel && (
        <div style={{ fontSize: 10, color: 'var(--text-faint,#62655a)', marginTop: 2 }}>
          {sublabel}
        </div>
      )}
    </div>
  );
}

function StatCard({ label, value, icon, color, sub }: {
  label: string; value: string | number; icon: React.ReactNode; color: string; sub?: string;
}) {
  return (
    <div style={{
      flex: 1, minWidth: 0, padding: '8px 10px', borderRadius: 8,
      border: '1px solid var(--border,#262920)', background: 'var(--bg-2,#131410)',
      display: 'flex', alignItems: 'center', gap: 8,
    }}>
      <div style={{
        width: 28, height: 28, borderRadius: 6, display: 'flex', alignItems: 'center', justifyContent: 'center',
        background: color + '18', color, flexShrink: 0,
      }}>
        {icon}
      </div>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 16, fontWeight: 700, color: 'var(--text)', lineHeight: 1.2 }}>{value}</div>
        <div style={{ fontSize: 9, color: 'var(--text-faint)', lineHeight: 1.3 }}>{label}</div>
        {sub && <div style={{ fontSize: 8, color: 'var(--text-dim)', lineHeight: 1.2 }}>{sub}</div>}
      </div>
    </div>
  );
}

function BudgetBar({ used, limit, locale }: { used: number; limit: number; locale: Locale }) {
  const pct = Math.min(100, (used / limit) * 100);
  const over = pct > 100;
  return (
    <div style={{
      padding: '6px 10px', borderRadius: 6,
      border: '1px solid var(--border,#262920)', background: 'var(--bg-2,#131410)',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
        <span style={{ fontSize: 10, color: 'var(--text-faint)' }}>
          {t(locale, 'aiWorkbench.skills.budget')}
        </span>
        <span style={{ fontSize: 10, color: over ? 'var(--danger)' : 'var(--text-dim)' }}>
          {used.toLocaleString()} / {limit.toLocaleString()}
        </span>
      </div>
      <div style={{ height: 4, borderRadius: 2, background: 'var(--bg-3,#1a1b16)', overflow: 'hidden' }}>
        <div style={{
          height: '100%', borderRadius: 2, transition: 'width 0.3s',
          width: `${Math.min(pct, 100)}%`,
          background: over ? 'var(--danger)' : pct > 80 ? 'var(--warning)' : 'var(--accent,#cdf24b)',
        }} />
      </div>
      {over && (
        <div style={{ fontSize: 9, color: 'var(--danger)', marginTop: 3, display: 'flex', alignItems: 'center', gap: 3 }}>
          <AlertTriangle size={10} /> {t(locale, 'aiWorkbench.skills.budgetWarning')}
        </div>
      )}
    </div>
  );
}

// ── Helpers ──

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

function formatRelativeTime(createdAt: string): string {
  if (!createdAt) return '';
  try {
    const date = new Date(createdAt.endsWith('Z') ? createdAt : createdAt + 'Z');
    const diffMs = Date.now() - date.getTime();
    const diffMin = Math.floor(diffMs / 60000);
    if (diffMin < 1) return 'now';
    if (diffMin < 60) return `${diffMin}m`;
    const diffHr = Math.floor(diffMin / 60);
    if (diffHr < 24) return `${diffHr}h`;
    const diffDay = Math.floor(diffHr / 24);
    if (diffDay < 30) return `${diffDay}d`;
    return date.toLocaleDateString();
  } catch {
    return '';
  }
}

/** 计算下一个周三 18:00 的格式化字符串 */
function formatNextWednesday(): string {
  const now = new Date();
  const day = now.getDay(); // 0=Sun, 3=Wed
  const daysUntilWed = (3 - day + 7) % 7;
  const nextWed = new Date(now);
  nextWed.setDate(now.getDate() + (daysUntilWed === 0 ? 7 : daysUntilWed));
  nextWed.setHours(18, 0, 0, 0);
  const days = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];
  return `${days[nextWed.getDay()]} ${String(nextWed.getHours()).padStart(2, '0')}:${String(nextWed.getMinutes()).padStart(2, '0')}`;
}
