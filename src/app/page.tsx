'use client';

import { useCallback, useEffect, useState } from 'react';
import { Terminal, Package, Settings, Square, HardDrive, Database } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { useRecentModules } from '@/lib/recent-modules';

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

  // LRU list of recently-opened module ids (ISSUE-3). Reactive: updates when
  // ShellLayout records a module open via pushRecentModule.
  const { ids: recentIds } = useRecentModules(8);

  useEffect(() => {
    async function loadData() {
      setLoading(true);
      try {
        const api = window.nativesAPI;
        if (!api) return;

        const savedLocale = await api.getLocale().catch(() => null);
        if (savedLocale === 'en') setLocale('en');

        // App version from package.json (ISSUE-2) — single source of truth.
        let version = '...';
        try {
          if (api.app?.version) version = await api.app.version();
        } catch { /* keep '...' */ }

        // Load modules — use list() for full metadata (enabled/version).
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

        // DB health check — try to read a known key
        let dbOk = false;
        try {
          await api.db.get('__health_check__');
          dbOk = true;
        } catch {
          dbOk = false;
        }

        // Disk usage of ~/.natives/
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

        // Recent notifications (last 5) + unread count (ISSUE-1, US20).
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
        } catch { /* notifications unavailable in dev mode */ }

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

  // Derive "recently used" modules from the LRU id list, intersected with
  // actually-installed modules (ISSUE-3). Falls back to all modules when no
  // access history exists yet.
  useEffect(() => {
    if (allModules.length === 0) {
      setRecentModules([]);
      return;
    }
    const byId = new Map(allModules.map((m) => [m.id, m]));
    const ordered = recentIds
      .map((id) => byId.get(id))
      .filter((m): m is ModuleInfo => Boolean(m));
    // If no history yet, show first 5 installed modules as a graceful default.
    setRecentModules(ordered.length > 0 ? ordered : allModules.slice(0, 5));
  }, [recentIds, allModules]);

  const handleOpenTerminal = useCallback(() => {
    window.dispatchEvent(new CustomEvent('toggle-terminal'));
  }, []);

  const handleInstallModule = useCallback(() => {
    // P0-4: Redirect to Workshop for permission-gated install
    window.dispatchEvent(new CustomEvent('navigate', { detail: '__workshop__' }));
  }, []);

  const handleSettings = useCallback(() => {
    window.dispatchEvent(new CustomEvent('navigate', { detail: '__settings__' }));
  }, []);

  const handleOpenFiles = useCallback(() => {
    window.dispatchEvent(new CustomEvent('navigate', { detail: 'files' }));
  }, []);

  const handleOpenWorkshop = useCallback(() => {
    window.dispatchEvent(new CustomEvent('navigate', { detail: '__workshop__' }));
  }, []);

  const handleOpenReleaseWizard = useCallback(() => {
    window.dispatchEvent(new CustomEvent('open-release-wizard'));
  }, []);

  const handleCheckUpdates = useCallback(() => {
    window.dispatchEvent(new CustomEvent('check-updates'));
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
          <span style={{ fontSize: 12, color: '#d9534f' }}>⚠ {error}</span>
          <button
            onClick={() => { setError(null); setRetryCount(c => c + 1); }}
            style={{
              padding: '4px 12px', borderRadius: 4, border: '1px solid #d9534f44',
              background: 'transparent', color: '#d9534f', fontSize: 11, cursor: 'pointer',
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
          valueColor={loading ? 'var(--text-faint)' : status.dbOk ? 'var(--accent,#cdf24b)' : '#d9534f'}
        />
        <StatusCard
          icon={<Package size={16} />}
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
          icon={<Terminal size={16} />}
          label={t(locale, 'settings.version')}
          value={status.version}
        />
      </div>

      {/* Quick Actions */}
      <div style={{ ...cardStyle, marginBottom: 24 }}>
        <div style={sectionTitleStyle}>{t(locale, 'dashboard.quickActions')}</div>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <ActionButton icon={<Terminal size={14} />} label={t(locale, 'dashboard.openTerminal')} onClick={handleOpenTerminal} primary />
          <ActionButton icon={<Package size={14} />} label={t(locale, 'dashboard.installModule')} onClick={handleInstallModule} />
          <ActionButton icon="📁" label={t(locale, 'dashboard.fileBrowser')} onClick={handleOpenFiles} />
          <ActionButton icon="🔧" label={t(locale, 'nav.workshop')} onClick={handleOpenWorkshop} />
          <ActionButton icon={<Settings size={14} />} label={t(locale, 'dashboard.openSettings')} onClick={handleSettings} />
          <ActionButton icon="🚀" label={t(locale, 'release.title')} onClick={handleOpenReleaseWizard} />
          <ActionButton icon="🔄" label={t(locale, 'update.title')} onClick={handleCheckUpdates} />
        </div>
      </div>

      {/* Recent Modules (ISSUE-3: ordered by LRU access time) */}
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

      {/* Recent Activity (ISSUE-1: notifications, ported from dead Dashboard.tsx) */}
      {notifications.length > 0 && (
        <div style={cardStyle}>
          <div style={{ ...sectionTitleStyle, display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <span>{t(locale, 'dashboard.recentActivity')}</span>
            {status.unreadNotifications > 0 && (
              <span style={{
                fontSize: 10, padding: '1px 6px', borderRadius: 9,
                background: '#e06a5b22', color: '#e06a5b',
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
                <span style={{ fontSize: 10, flexShrink: 0 }}>
                  {notif.level === 'error' ? '🔴' : notif.level === 'warning' ? '🟡' : '🔵'}
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

function ActionButton({ icon, label, onClick, primary }: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  primary?: boolean;
}) {
  return (
    <button
      className={`btn ${primary ? 'btn-primary' : ''}`}
      onClick={onClick}
      style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12 }}
    >
      {icon}
      <span>{label}</span>
    </button>
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

// Render a SQLite datetime (UTC, "YYYY-MM-DD HH:MM:SS") as a short relative
// label like "2m" / "3h" / "2d", falling back to the locale date string.
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
