'use client';

import { useState } from 'react';
import type { Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';

interface Session {
  id: string;
  project_id?: string | null;
  title: string;
  model_id: string;
  provider_id: string;
  created_at: string;
  updated_at: string;
  message_count: number;
}

interface SessionListProps {
  sessions: Session[];
  activeSessionId: string | null;
  onSessionSelect: (id: string) => void;
  onSessionDelete: (id: string) => void;
  onNewSession: () => void;
  locale: Locale;
  loading?: boolean;
}

export default function SessionList({
  sessions,
  activeSessionId,
  onSessionSelect,
  onSessionDelete,
  onNewSession,
  locale,
  loading = false,
}: SessionListProps) {
  const [deleteTarget, setDeleteTarget] = useState<Session | null>(null);
  const t = (key: string) => {
    const lang = locale.startsWith('zh') ? 'zh' : 'en';
    const labels: Record<string, Record<string, string>> = {
      newSession: { zh: '新建会话', en: 'New Session' },
      noSessions: { zh: '暂无会话', en: 'No sessions' },
      loading: { zh: '加载中...', en: 'Loading...' },
      delete: { zh: '删除', en: 'Delete' },
      draft: { zh: '全局草稿', en: 'Draft' },
      confirmDelete: { zh: '确定删除此会话？', en: 'Delete this session?' },
    };
    return labels[key]?.[lang] ?? key;
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-3 py-3">
        <button
          onClick={onNewSession}
          className="w-full flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--primary-soft)] text-[var(--primary)] text-sm font-medium hover:opacity-80 transition-opacity"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
          <span>{t('newSession')}</span>
        </button>
      </div>

      <div className="flex-1 overflow-y-auto px-2 space-y-0.5">
        {loading ? (
          <div className="px-3 py-8 text-center text-xs text-[var(--text-disabled)]">{t('loading')}</div>
        ) : sessions.length === 0 ? (
          <div className="px-3 py-8 text-center text-xs text-[var(--text-disabled)]">{t('noSessions')}</div>
        ) : (
          sessions.map((session) => (
            <div
              key={session.id}
              className={`group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer text-sm transition-all ${
                activeSessionId === session.id
                  ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                  : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
              }`}
              onClick={() => onSessionSelect(session.id)}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="shrink-0 opacity-50">
                <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" />
              </svg>
              <span className="truncate flex-1">{session.title || t('draft')}</span>
              <span className="text-[0.625rem] text-[var(--text-disabled)] opacity-0 group-hover:opacity-100 transition-opacity">
                {session.message_count}
              </span>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setDeleteTarget(session);
                }}
                className="shrink-0 opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-red-500/10 text-red-400 transition-all"
                title={t('delete')}
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <polyline points="3 6 5 6 21 6" />
                  <path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2" />
                </svg>
              </button>
            </div>
          ))
        )}
      </div>

      {deleteTarget && (
        <ConfirmDialog
          open={!!deleteTarget}
          title={t('confirmDelete')}
          message={`${t('confirmDelete')} "${deleteTarget.title || t('draft')}"`}
          confirmLabel={t('delete')}
          cancelLabel="Cancel"
          danger
          onConfirm={() => {
            onSessionDelete(deleteTarget.id);
            setDeleteTarget(null);
          }}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  );
}
