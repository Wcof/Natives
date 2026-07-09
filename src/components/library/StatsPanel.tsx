'use client';

import type { ReactNode } from 'react';
import { useLocale, t as tr } from '@/i18n';
import { Archive, Folder, Tags, Clock3 } from 'lucide-react';

interface LibraryStats { totalItems: number; totalFolders: number; totalTags: number; itemsByFolder: { folderId: string | null; folderName: string; count: number }[]; recentItems: number; }

export function StatsPanel({ stats }: { stats: LibraryStats | null }) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  if (!stats) return null;

  const maxCount = Math.max(...stats.itemsByFolder.map(f => f.count), 1);
  const refreshedAt = new Date().toLocaleString(locale === 'zh' ? 'zh-CN' : 'en-US');

  return (
    <div className="space-y-6">
      <h2 className="text-lg font-semibold">{t('library.statistics')}</h2>

      {/* Overview cards */}
      <div className="grid grid-cols-2 gap-3">
        <StatCard label={t('library.totalItems')} value={stats.totalItems} icon={<Archive size={14} />} />
        <StatCard label={t('library.totalFolders')} value={stats.totalFolders} icon={<Folder size={14} />} />
        <StatCard label={t('library.totalTags')} value={stats.totalTags} icon={<Tags size={14} />} />
        <StatCard label={t('library.recentItems')} value={stats.recentItems} icon={<Clock3 size={14} />} />
      </div>

      {/* Items by folder */}
      <div>
        <h3 className="mb-2 text-sm font-medium" style={{ color: 'var(--text-secondary)' }}>{t('library.itemsByFolder')}</h3>
        <div className="space-y-1.5">
          {stats.itemsByFolder.map((f) => (
            <div key={f.folderId ?? '__unassigned'} className="flex items-center gap-2">
              <span className="text-xs w-32 truncate text-right">{f.folderName}</span>
              <div className="h-4 flex-1 overflow-hidden rounded" style={{ background: 'var(--surface)' }}>
                <div
                  className="h-full rounded transition-all"
                  style={{ width: `${(f.count / maxCount) * 100}%`, background: 'var(--accent)' }}
                />
              </div>
              <span className="text-xs font-mono w-8 text-left">{f.count}</span>
            </div>
          ))}
        </div>
      </div>

      {/* Summary */}
      <div className="border-t pt-2 text-xs" style={{ borderColor: 'var(--border)', color: 'var(--text-disabled)' }}>
        {t('library.statsRefreshedAt')} {refreshedAt}
      </div>
    </div>
  );
}

function StatCard({ label, value, icon }: { label: string; value: number; icon: ReactNode }) {
  return (
    <div className="rounded-lg border p-3" style={{ borderColor: 'var(--border)', background: 'var(--surface)' }}>
      <div className="mb-1 flex items-center gap-1.5 text-xs" style={{ color: 'var(--text-secondary)' }}>
        <span style={{ color: 'var(--accent)' }}>{icon}</span>
        <span>{label}</span>
      </div>
      <p className="text-xl font-bold" style={{ color: 'var(--text)' }}>{value}</p>
    </div>
  );
}
