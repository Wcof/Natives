'use client';

import type { Locale } from '@/i18n';

interface ConfirmOverwriteModalProps {
  isOpen: boolean;
  moduleName: string;
  locale: Locale;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmOverwriteModal({
  isOpen,
  moduleName,
  locale,
  onConfirm,
  onCancel,
}: ConfirmOverwriteModalProps) {
  if (!isOpen) return null;

  const t = (key: string) => {
    const lang = locale.startsWith('zh') ? 'zh' : 'en';
    const labels: Record<string, Record<string, string>> = {
      title: { zh: '检测到该应用正在运行', en: 'Active Instance Detected' },
      body: { zh: `"${moduleName}" 正在运行，是否强制关闭并应用新版本？`, en: `"${moduleName}" is running. Force close and apply new version?` },
      confirm: { zh: '确认覆盖', en: 'Confirm Overwrite' },
      cancel: { zh: '取消', en: 'Cancel' },
    };
    return labels[key]?.[lang] ?? key;
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/40" onClick={onCancel} />
      <div className="relative rounded-xl border border-[var(--border)] bg-[var(--surface)] p-6 shadow-modal max-w-md w-full mx-4">
        <div className="flex items-center gap-3 mb-4">
          <div className="w-10 h-10 rounded-xl bg-amber-500/20 flex items-center justify-center">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-amber-400">
              <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z" />
              <line x1="12" y1="9" x2="12" y2="13" />
              <line x1="12" y1="17" x2="12.01" y2="17" />
            </svg>
          </div>
          <div>
            <h3 className="text-sm font-semibold text-[var(--text)]">{t('title')}</h3>
            <p className="text-xs text-[var(--text-secondary)] mt-1">{t('body')}</p>
          </div>
        </div>
        <div className="flex justify-end gap-2 mt-6">
          <button
            onClick={onCancel}
            className="px-4 py-2 rounded-lg text-sm text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] transition-all"
          >
            {t('cancel')}
          </button>
          <button
            onClick={onConfirm}
            className="px-4 py-2 rounded-lg text-sm font-medium bg-amber-500/20 text-amber-400 hover:bg-amber-500/30 transition-all"
          >
            {t('confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}
