'use client';

import { type DeleteImpact } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import { AlertCircle } from 'lucide-react';

interface DeleteImpactModalProps {
  impact: DeleteImpact;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteImpactModal({ impact, onConfirm, onCancel }: DeleteImpactModalProps) {
  const locale = useLocale();

  return (
    <div className="fixed inset-0 bg-[var(--background)]/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-[var(--card)] border border-[var(--border)] rounded-2xl w-full max-w-md p-6 shadow-2xl space-y-4">
        <h3 className="font-bold text-lg text-[var(--destructive)] flex items-center gap-2">
          <AlertCircle className="w-5 h-5" />
          {t(locale, 'aiResources.deleteImpactTitle')}
        </h3>
        <p className="text-sm text-[var(--muted-foreground)]">
          {t(locale, 'aiResources.deleteImpactDesc')}
        </p>
        <div className="p-3 bg-[var(--secondary)]/40 rounded-xl space-y-1 text-xs">
          <div>• Connections: {impact.connectionCount}</div>
          <div>• Credentials: {impact.credentialCount}</div>
          <div>• Models: {impact.modelCount}</div>
          <div>• Affected Routes: {impact.affectedRouteCount}</div>
        </div>
        <div className="flex justify-end gap-2 pt-3">
          <button
            onClick={onCancel}
            className="px-4 py-2 bg-[var(--secondary)] text-[var(--secondary-foreground)] rounded-lg text-sm font-medium"
          >
            {t(locale, 'aiResources.cancel')}
          </button>
          <button
            onClick={onConfirm}
            className="px-4 py-2 bg-[var(--destructive)] text-[var(--primary-foreground)] rounded-lg text-sm font-medium shadow-sm"
          >
            {t(locale, 'aiResources.confirmDelete')}
          </button>
        </div>
      </div>
    </div>
  );
}
