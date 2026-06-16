import { useState, useEffect } from 'react';
import { en } from './en';
import { zh } from './zh';

export type Locale = 'en' | 'zh' | 'zh-CN';

export const locales: Record<string, typeof en> = { en, zh, 'zh-CN': zh };

export function t(locale: string, key: string): string {
  const normLocale = (locale && locale.startsWith('zh')) ? 'zh' : 'en';
  const keys = key.split('.');
  let result: unknown = locales[normLocale];
  for (const k of keys) {
    if (result && typeof result === 'object') {
      result = (result as Record<string, unknown>)[k];
    } else {
      return key;
    }
  }
  return typeof result === 'string' ? result : key;
}

export function useLocale(): Locale {
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    let cancelled = false;
    const refreshLocale = async (e?: Event) => {
      const customEvent = e as CustomEvent<string>;
      const newLocale = customEvent?.detail;
      if (newLocale) {
        if (!cancelled) {
          setLocale(newLocale === 'en' ? 'en' : 'zh');
        }
      } else {
        try {
          const saved = await window.nativesAPI?.getLocale?.();
          if (!cancelled && saved) {
            setLocale(saved === 'en' ? 'en' : 'zh');
          }
        } catch { /* ignore */ }
      }
    };

    refreshLocale();
    window.addEventListener('locale-changed', refreshLocale as EventListener);
    return () => {
      cancelled = true;
      window.removeEventListener('locale-changed', refreshLocale as EventListener);
    };
  }, []);
  return locale;
}
