'use client';

import { useCallback, useState } from 'react';
import { Clock3, FolderGit2, RotateCcw, Terminal as TerminalIcon } from 'lucide-react';
import { t as tr, useLocale } from '@/i18n';
import type { AgentProject, AgentSession } from '@/types/agent';
import { useAsyncData } from '@/hooks/useAsyncData';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { FILE_EVENTS, dispatchFileEvent } from '@/lib/file-events';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

/**
 * Agent 会话（合并原 SessionReplay + ProjectMemory 两个假面板）：
 * 数据源为真实的 Claude Code 会话存储 ~/.claude/projects/<slug>/*.jsonl
 * （src-tauri/src/agent.rs scan_sessions）。恢复动作 = 复制
 * `cd <project> && claude --resume <id>` 到剪贴板并展开终端面板——
 * 不再创建游离于终端面板之外的孤儿 PTY。
 */

interface RawProject { path: string; name: string; hasGit: boolean; languages: string[] }

export default function AgentSessions() {
  const locale = useLocale();
  const t = useCallback((key: string, params?: Record<string, string | number>) => tr(locale, key, params), [locale]);
  const { toast } = useToast();

  const [selectedProject, setSelectedProject] = useState<AgentProject | null>(null);

  const projectsState = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (!api?.agent?.scanProjects) throw new Error('Agent API unavailable');
    const raw = (await api.agent.scanProjects()) as RawProject[];
    return raw.map((p): AgentProject => ({
      path: p.path,
      name: p.name,
      engine: 'claude',
      lastActive: 0,
      sessionCount: 0,
    }));
  }, []);

  const sessionsState = useAsyncData(async () => {
    if (!selectedProject) return [] as AgentSession[];
    const api = window.nativesAPI;
    if (!api?.agent?.getSessions) throw new Error('Agent API unavailable');
    return (await api.agent.getSessions(selectedProject.path)) as AgentSession[];
  }, [selectedProject]);

  const handleResume = useCallback(async (session: AgentSession) => {
    if (!selectedProject) return;
    const command = `cd ${JSON.stringify(selectedProject.path)} && claude --resume ${session.id}`;
    try {
      await window.nativesAPI?.clipboard?.write?.(command);
      dispatchFileEvent(FILE_EVENTS.openTerminal);
      toast(t('aiWorkbench.sessions.resumeCopied'), 'success');
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    }
  }, [selectedProject, toast, t]);

  const formatTime = (ms: number) => {
    if (!ms) return '—';
    try { return new Date(ms).toLocaleString(locale === 'zh' ? 'zh-CN' : 'en-US'); } catch { return '—'; }
  };

  const formatSize = (bytes: number) => {
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${bytes} B`;
  };

  if (projectsState.loading) return <LoadingState message={t('common.loading')} />;
  if (projectsState.error) {
    return <ErrorState message={projectsState.error.userMessage} onRetry={projectsState.reload} />;
  }

  const projects = projectsState.data ?? [];
  if (projects.length === 0) {
    return <EmptyState title={t('aiWorkbench.sessions.noProjects')} />;
  }

  return (
    <div style={{ display: 'flex', height: '100%', gap: SPACING.md }}>
      {/* Left: projects */}
      <div style={{ width: 220, flexShrink: 0, overflow: 'auto', borderRight: '1px solid var(--border)', paddingRight: SPACING.sm }}>
        <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 0.5, padding: '4px 6px' }}>
          {t('aiWorkbench.sessions.projects')}
        </div>
        {projects.map((p) => (
          <button
            key={p.path}
            type="button"
            onClick={() => setSelectedProject(p)}
            aria-current={selectedProject?.path === p.path ? 'true' : undefined}
            style={{
              display: 'flex', alignItems: 'center', gap: 6, width: '100%',
              padding: '6px 8px', marginBottom: 2, borderRadius: BORDER_RADIUS.sm,
              border: 'none', cursor: 'pointer', textAlign: 'left',
              fontSize: FONT_SIZE.sm,
              background: selectedProject?.path === p.path ? 'var(--accent-soft)' : 'transparent',
              color: selectedProject?.path === p.path ? 'var(--accent)' : 'var(--text)',
            }}
            title={p.path}
          >
            <FolderGit2 size={13} style={{ flexShrink: 0 }} />
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{p.name}</span>
          </button>
        ))}
      </div>

      {/* Right: sessions of selected project */}
      <div style={{ flex: 1, overflow: 'auto' }}>
        {!selectedProject ? (
          <EmptyState title={t('aiWorkbench.sessions.selectProject')} />
        ) : sessionsState.loading ? (
          <LoadingState message={t('common.loading')} />
        ) : sessionsState.error ? (
          <ErrorState message={sessionsState.error.userMessage} onRetry={sessionsState.reload} />
        ) : (sessionsState.data ?? []).length === 0 ? (
          <EmptyState title={t('aiWorkbench.sessions.noSessions')} description={t('aiWorkbench.sessions.noSessionsDesc')} />
        ) : (
          (sessionsState.data ?? []).map((s) => (
            <div
              key={s.id}
              style={{
                display: 'flex', alignItems: 'center', gap: SPACING.sm,
                padding: '8px 10px', marginBottom: 4,
                borderRadius: BORDER_RADIUS.md,
                border: '1px solid var(--border)', background: 'var(--surface)',
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{
                  fontSize: FONT_SIZE.sm, fontWeight: 500, color: 'var(--text)',
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                }}>
                  {s.title || t('aiWorkbench.sessions.untitled')}
                </div>
                <div style={{
                  display: 'flex', alignItems: 'center', gap: 10,
                  fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', marginTop: 2,
                }}>
                  <span style={{ display: 'inline-flex', alignItems: 'center', gap: 3 }}>
                    <Clock3 size={11} /> {formatTime(s.mtimeMs)}
                  </span>
                  <span>{formatSize(s.size)}</span>
                  <span style={{ fontFamily: 'var(--font-mono)' }}>{s.id.slice(0, 8)}</span>
                </div>
              </div>
              <button
                type="button"
                className="btn btn-sm"
                onClick={() => handleResume(s)}
                title={t('aiWorkbench.sessions.resumeHint')}
                style={{ display: 'inline-flex', alignItems: 'center', gap: 4, flexShrink: 0 }}
              >
                <RotateCcw size={12} />
                <TerminalIcon size={12} />
                {t('aiWorkbench.sessions.resume')}
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
