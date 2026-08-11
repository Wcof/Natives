'use client';

// WorktreeChip（UX-13/14 · W8）— 会话头部 linked-worktree 只读指示器。
//
// 只读：resolveLinkedWorktree 复用 git.status（分支名）+ fs.readFile(.git)
// （linked worktree 判定），不创建/删除/prune/repair/切换分支。
//
// 状态诚实：
//   - 无绑定项目 / 能力不可用 → 不渲染
//   - 解析中            → 加载态（aria-busy）
//   - linked worktree  → 显示路径 chip（可访问名含完整路径 + 分支）
//   - 主 worktree / 非 git → 显示「未绑定」诚实空态
//   - 解析出错          → 显示「未知」诚实错误态
// reduced-motion：静态展示，无动画。

import { useEffect, useState } from 'react';
import { GitBranch } from 'lucide-react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import {
  resolveLinkedWorktree,
  type LinkedWorktreeResolution,
} from '@/lib/linked-worktree';

export interface WorktreeChipProps {
  locale: Locale;
  /** 当前会话绑定的 cwd/project_path（null = 未绑定）。 */
  projectPath: string | null;
}

function pathBasename(p: string): string {
  const trimmed = p.replace(/[\\/]+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
}

const chipStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  maxWidth: 220,
  padding: '2px 8px',
  borderRadius: 999,
  border: '1px solid var(--border)',
  background: 'var(--bg-soft)',
  fontSize: 11,
  color: 'var(--text-secondary)',
  lineHeight: 1.5,
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
};

export function WorktreeChip({ locale, projectPath }: WorktreeChipProps) {
  const [resolution, setResolution] = useState<LinkedWorktreeResolution | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!projectPath) {
      setResolution({
        state: 'none',
        projectPath: null,
        branch: null,
        worktreePath: null,
        gitDir: null,
        error: null,
      });
      return undefined;
    }
    setResolution(null); // 进入加载态
    void resolveLinkedWorktree(projectPath).then((res) => {
      if (!cancelled) setResolution(res);
    });
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  if (!projectPath) return null;
  if (!resolution) {
    // 加载态
    return (
      <span
        style={chipStyle}
        aria-busy="true"
        aria-label={t(locale, 'assistant.worktreeChecking')}
        title={t(locale, 'assistant.worktreeChecking')}
      >
        <GitBranch size={12} aria-hidden="true" />
        <span style={{ opacity: 0.7 }}>…</span>
      </span>
    );
  }

  if (resolution.state === 'unavailable' || resolution.state === 'none') {
    return null;
  }

  if (resolution.state === 'linked' && resolution.worktreePath) {
    const label = t(locale, 'assistant.worktreeChipDetail', {
      path: resolution.worktreePath,
      branch: resolution.branch ?? '?',
    });
    return (
      <span
        style={{ ...chipStyle, borderColor: 'var(--accent)', color: 'var(--text)' }}
        title={`${resolution.worktreePath}${resolution.branch ? ` · ${resolution.branch}` : ''}`}
        aria-label={label}
      >
        <GitBranch size={12} aria-hidden="true" style={{ color: 'var(--accent)', flexShrink: 0 }} />
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>
          {pathBasename(resolution.worktreePath)}
        </span>
      </span>
    );
  }

  if (resolution.state === 'error') {
    return (
      <span
        style={{ ...chipStyle, borderColor: 'var(--warning)', color: 'var(--warning)' }}
        title={resolution.error ?? undefined}
        aria-label={t(locale, 'assistant.worktreeUnknown')}
      >
        {t(locale, 'assistant.worktreeUnknown')}
      </span>
    );
  }

  // main worktree / notGit → 「未绑定」诚实空态
  return (
    <span
      style={chipStyle}
      title={t(locale, 'assistant.worktreeUnboundTitle')}
      aria-label={t(locale, 'assistant.worktreeUnbound')}
    >
      {t(locale, 'assistant.worktreeUnbound')}
    </span>
  );
}

export default WorktreeChip;
