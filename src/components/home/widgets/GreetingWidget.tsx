'use client';

/**
 * 问候 Widget（默认 5 Widget 之一）。
 * 只消费现有设置（settings:username），无 IPC 轮询、无 Timer。
 */

import { useEffect, useState } from 'react';
import { t, useLocale } from '@/i18n';

export function GreetingWidget() {
  const locale = useLocale();
  const [username, setUsername] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const api = window.nativesAPI;
        if (!api?.db?.get) return;
        const value = await api.db.get('settings:username');
        if (!cancelled) setUsername(typeof value === 'string' && value.trim() ? value.trim() : null);
      } catch {
        /* browser dev mode */
      }
    };
    void load();
    return () => { cancelled = true; };
  }, []);

  return (
    <div className="flex h-full flex-col justify-center gap-1 px-1">
      <div className="text-base font-semibold text-[var(--text)]">
        {t(locale, 'home.greeting', { name: username ?? t(locale, 'home.guest') })}
      </div>
      <div className="text-xs text-[var(--text-secondary)]">{t(locale, 'home.greetingSub')}</div>
    </div>
  );
}
