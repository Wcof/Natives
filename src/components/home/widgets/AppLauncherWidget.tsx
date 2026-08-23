'use client';

/**
 * 常用应用 Widget（默认 5 Widget 之一）。
 * 使用 `appsApi.listViews()`（统一 applications 注册表），不直接查 SQLite / 进程。
 */

import { useEffect, useState } from 'react';
import { Box } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { appsApi, type AppView } from '@/lib/tauri/apps';

const LIMIT = 8;

export function AppLauncherWidget() {
  const locale = useLocale();
  const [apps, setApps] = useState<AppView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const list = await appsApi.listViews();
        if (!cancelled) setApps(list.slice(0, LIMIT));
      } catch {
        if (!cancelled) setError('unavailable');
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
        {t(locale, 'home.appsUnavailable')}
      </div>
    );
  }

  if (apps === null) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
        {t(locale, 'common.loading')}
      </div>
    );
  }

  if (apps.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
        {t(locale, 'home.noApps')}
      </div>
    );
  }

  return (
    <ul className="flex h-full flex-col gap-1 overflow-y-auto px-1">
      {apps.map((app) => (
        <li key={app.appId}>
          <div
            title={app.title}
            className="flex w-full items-center gap-2 rounded-lg px-2 py-1 text-left text-xs text-[var(--text-secondary)]"
          >
            <Box size={13} className="shrink-0 text-[var(--text-disabled)]" />
            <span className="min-w-0 flex-1 truncate">{app.title}</span>
            {app.runtimeState === 'running' ? (
              <span className="shrink-0 rounded bg-[var(--primary-soft)] px-1.5 py-0.5 text-[0.625rem] text-[var(--primary)]">
                {t(locale, 'home.running')}
              </span>
            ) : null}
          </div>
        </li>
      ))}
    </ul>
  );
}
