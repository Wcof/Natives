'use client';

import React from 'react';
import {
  Play,
  Square,
  RotateCw,
  ExternalLink,
  Edit2,
  Trash2,
  Bookmark,
  BookmarkCheck,
  Eraser,
} from 'lucide-react';
import { t, useLocale } from '@/i18n';
import type { AppView } from '@/lib/tauri/apps';

interface AppActionBarProps {
  app: AppView;
  loading?: boolean;
  onOpen: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onToggleSidebar: () => void;
  onClearData?: () => void;
}

export function AppActionBar({
  app,
  loading = false,
  onOpen,
  onStart,
  onStop,
  onRestart,
  onEdit,
  onRemove,
  onToggleSidebar,
  onClearData,
}: AppActionBarProps) {
  const locale = useLocale();
  const { capabilities: caps, runtimeState } = app;
  const isRunning = runtimeState === 'running';

  return (
    <div className="flex flex-wrap items-center gap-2">
      {/* Open Button */}
      {caps.canOpen && (
        <button
          type="button"
          onClick={onOpen}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-[var(--primary)] text-[var(--primary-foreground)] text-xs font-medium hover:bg-[var(--primary-hover)] transition-colors shadow-sm"
        >
          <ExternalLink className="h-3.5 w-3.5" />
          {t(locale, 'appsPage.open')}
        </button>
      )}

      {/* Start Button */}
      {caps.canStart && !isRunning && (
        <button
          type="button"
          onClick={onStart}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-[var(--success)] text-[var(--primary-foreground)] text-xs font-medium hover:opacity-90 transition-opacity shadow-sm"
        >
          <Play className="h-3.5 w-3.5" />
          {t(locale, 'appsPage.start')}
        </button>
      )}

      {/* Restart Button */}
      {caps.canRestart && (
        <button
          type="button"
          onClick={onRestart}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-[var(--border)] bg-[var(--surface)] text-[var(--text-secondary)] hover:text-[var(--text)] hover:border-[var(--border-strong)] text-xs font-medium transition-colors"
        >
          <RotateCw className="h-3.5 w-3.5" />
          {t(locale, 'appsPage.restart')}
        </button>
      )}

      {/* Stop Button */}
      {caps.canStop && (
        <button
          type="button"
          onClick={onStop}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-[var(--danger)]/20 bg-[var(--danger-soft)] text-[var(--danger)] hover:opacity-80 text-xs font-medium transition-colors"
        >
          <Square className="h-3.5 w-3.5" />
          {t(locale, 'appsPage.stop')}
        </button>
      )}


      {/* Clear Web Data (Web App) */}
      {app.kind === 'web_application' && onClearData && (
        <button
          type="button"
          onClick={onClearData}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-[var(--warning)]/20 bg-[var(--warning-soft)] text-[var(--warning)] hover:opacity-80 text-xs font-medium transition-colors"
        >
          <Eraser className="h-3.5 w-3.5" />
          {t(locale, 'appsPage.clearData')}
        </button>
      )}

      <div className="h-4 w-px bg-[var(--border-subtle)] mx-1" />

      {/* Sidebar Visibility Toggle */}
      {caps.canSidebar && (
        <button
          type="button"
          onClick={onToggleSidebar}
          disabled={loading}
          className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl border text-xs font-medium transition-colors ${
            app.showInSidebar
              ? 'border-[var(--primary)] bg-[var(--primary-soft)] text-[var(--primary)]'
              : 'border-[var(--border)] bg-[var(--surface)] text-[var(--text-tertiary)] hover:text-[var(--text)]'
          }`}
        >
          {app.showInSidebar ? (
            <>
              <BookmarkCheck className="h-3.5 w-3.5" />
              {t(locale, 'appsPage.hideFromSidebar')}
            </>
          ) : (
            <>
              <Bookmark className="h-3.5 w-3.5" />
              {t(locale, 'appsPage.showInSidebar')}
            </>
          )}
        </button>
      )}

      {/* Edit Button */}
      {caps.canEdit && (
        <button
          type="button"
          onClick={onEdit}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-[var(--border)] bg-[var(--surface)] text-[var(--text-secondary)] hover:text-[var(--text)] text-xs font-medium transition-colors"
        >
          <Edit2 className="h-3.5 w-3.5" />
          {t(locale, 'appsPage.editApp')}
        </button>
      )}

      {/* Remove Button */}
      {caps.canRemove && (
        <button
          type="button"
          onClick={onRemove}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-[var(--danger)]/20 text-[var(--danger)] hover:bg-[var(--danger-soft)] text-xs font-medium transition-colors"
        >
          <Trash2 className="h-3.5 w-3.5" />
          {t(locale, 'appsPage.removeApp')}
        </button>
      )}
    </div>
  );
}
