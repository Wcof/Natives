'use client';

import { useCallback, useEffect, useState } from 'react';
import { Terminal, Package, Settings, Square, HardDrive, Database } from 'lucide-react';
import { t, type Locale } from '@/i18n';

interface ModuleInfo {
  id: string;
  name: string;
  icon?: string;
}

interface SystemStatus {
  dbOk: boolean;
  moduleCount: number;
  version: string;
  diskUsage: string;
}

export default function DashboardPage() {
  const [locale, setLocale] = useState<Locale>('zh');
  const [recentModules, setRecentModules] = useState<ModuleInfo[]>([]);
  const [status, setStatus] = useState<SystemStatus>({
    dbOk: false,
    moduleCount: 0,
    version: '0.1.0',
    diskUsage: '...',
  });
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    async function loadData() {
      setLoading(true);
      try {
        const api = window.nativesAPI;
        if (!api) return;

        const savedLocale = await api.getLocale().catch(() => null);
        if (savedLocale === 'en') setLocale('en');

        // Load modules
        const modules = await api.module.scan();
        let modCount = 0;
        if (Array.isArray(modules)) {
          const scanned = modules as Array<{ manifest?: { id: string; name: string; icon?: string } }>;
          const valid = scanned.filter((m) => m.manifest).map((m) => ({
            id: m.manifest!.id,
            name: m.manifest!.name,
            icon: m.manifest!.icon,
          }));
          modCount = valid.length;
          setRecentModules(valid.slice(0, 5));
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

        setStatus({
          dbOk,
          moduleCount: modCount,
          version: '0.1.0',
          diskUsage: diskUsageStr,
        });
      } catch {
        // Browser dev mode
      } finally {
        setLoading(false);
      }
    }
    loadData();
  }, []);

  const handleOpenTerminal = useCallback(() => {
    window.dispatchEvent(new CustomEvent('toggle-terminal'));
  }, []);

  const handleInstallModule = useCallback(() => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.zip';
    input.onchange = () => {
      const file = input.files?.[0];
      if (file) {
        window.nativesAPI?.module?.install?.((file as unknown as { path?: string }).path || file.name);
      }
    };
    input.click();
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

  return (
    <div style={{ height: '100%', overflow: 'auto', padding: '24px 28px' }}>
      {/* Header */}
      <div style={{ marginBottom: 28 }}>
        <h1 style={{ fontSize: 24, fontWeight: 600, color: 'var(--text)', margin: 0 }}>
          {t(locale, 'dashboard.title')}
        </h1>
        <p style={{ color: 'var(--text-faint)', fontSize: 13, marginTop: 4 }}>
          AI Steam Base
        </p>
      </div>

      {/* System Status Grid */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 12, marginBottom: 24 }}>
        <StatusCard
          icon={<Database size={16} />}
          label="Database"
          value={loading ? '...' : status.dbOk ? 'Connected' : 'Error'}
          valueColor={loading ? 'var(--text-faint)' : status.dbOk ? 'var(--accent,#cdf24b)' : '#d9534f'}
        />
        <StatusCard
          icon={<Package size={16} />}
          label={t(locale, 'dashboard.installedModules')}
          value={loading ? '...' : String(status.moduleCount)}
        />
        <StatusCard
          icon={<HardDrive size={16} />}
          label="Data Usage"
          value={loading ? '...' : status.diskUsage}
        />
        <StatusCard
          icon={<Terminal size={16} />}
          label={t(locale, 'settings.version')}
          value={status.version}
        />
      </div>

      {/* Quick Actions */}
      <div style={{
        padding: '16px 20px',
        border: '1px solid var(--border,#262920)',
        borderRadius: 8,
        marginBottom: 24,
      }}>
        <div style={{
          fontSize: 11, fontWeight: 600,
          color: 'var(--text-dim,#9b9d8c)',
          textTransform: 'uppercase', letterSpacing: 1,
          marginBottom: 12,
        }}>
          {t(locale, 'dashboard.quickActions')}
        </div>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <ActionButton icon={<Terminal size={14} />} label={t(locale, 'dashboard.openTerminal')} onClick={handleOpenTerminal} primary />
          <ActionButton icon={<Package size={14} />} label={t(locale, 'dashboard.installModule')} onClick={handleInstallModule} />
          <ActionButton icon="📁" label="File Browser" onClick={handleOpenFiles} />
          <ActionButton icon="🔧" label={t(locale, 'nav.workshop')} onClick={handleOpenWorkshop} />
          <ActionButton icon={<Settings size={14} />} label={t(locale, 'dashboard.openSettings')} onClick={handleSettings} />
          <ActionButton icon="🚀" label={t(locale, 'release.title')} onClick={handleOpenReleaseWizard} />
          <ActionButton icon="🔄" label={t(locale, 'update.title')} onClick={handleCheckUpdates} />
        </div>
      </div>

      {/* Recent Modules */}
      <div style={{
        padding: '16px 20px',
        border: '1px solid var(--border,#262920)',
        borderRadius: 8,
      }}>
        <div style={{
          fontSize: 11, fontWeight: 600,
          color: 'var(--text-dim,#9b9d8c)',
          textTransform: 'uppercase', letterSpacing: 1,
          marginBottom: 12,
        }}>
          {t(locale, 'dashboard.lastUsed')}
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
    </div>
  );
}

// ── Sub-components ──

function StatusCard({ icon, label, value, valueColor }: {
  icon: React.ReactNode;
  label: string;
  value: string;
  valueColor?: string;
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
