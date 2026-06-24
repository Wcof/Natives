'use client';

import type { Locale } from '@/i18n';

interface LoadingPageProps {
  moduleName: string;
  locale: Locale;
  onBack: () => void;
}

export default function LoadingPage({ moduleName, locale, onBack }: LoadingPageProps) {
  const t = (key: string) => {
    const lang = locale.startsWith('zh') ? 'zh' : 'en';
    const labels: Record<string, Record<string, string>> = {
      generating: { zh: `${moduleName} 生成中...`, en: `${moduleName} generating...` },
      back: { zh: '返回助理', en: 'Back to Assistant' },
      pleaseWait: { zh: '请稍候，模块正在构建中', en: 'Please wait, module is being built' },
    };
    return labels[key]?.[lang] ?? key;
  };

  return (
    <div className="flex-1 flex flex-col items-center justify-center gap-4">
      {/* Loading spinner */}
      <div className="relative w-16 h-16">
        <div className="absolute inset-0 rounded-full border-2 border-[var(--vibe-border-subtle)]" />
        <div className="absolute inset-0 rounded-full border-2 border-transparent border-t-purple-400 animate-spin" />
      </div>

      <div className="text-center">
        <h3 className="text-sm font-medium text-[var(--vibe-brand-text)]">{t('generating')}</h3>
        <p className="text-xs text-[var(--text-dim)] mt-1">{t('pleaseWait')}</p>
      </div>

      <button
        onClick={onBack}
        className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm text-[var(--text-dim)] hover:bg-[var(--vibe-btn-hover-bg)] transition-all"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <line x1="19" y1="12" x2="5" y2="12" />
          <polyline points="12 19 5 12 12 5" />
        </svg>
        <span>{t('back')}</span>
      </button>
    </div>
  );
}
