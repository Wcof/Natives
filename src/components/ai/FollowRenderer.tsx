'use client';

import { useEffect, useMemo, useState } from 'react';
import { Package } from 'lucide-react';
import { followPriority, getFollowState } from '@/lib/follow-mode';
import { getScrollbackLines } from '@/lib/path-detector';
import { parseAgentAction, composeNarration } from '@/lib/agent-narration';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { t, useLocale } from '@/i18n';
import PreviewSurface from '@/components/preview/PreviewSurface';
import { createBuiltinRegistry, createDefaultContext } from '@/lib/preview/composition';
import { PreviewService } from '@/lib/preview/service';

interface FollowRendererProps {
  filePath: string | null;
}

/**
 * FollowRenderer — 文件跟随（file-follow）只读呈现。
 *
 * 文件内容统一走 Preview Capability（PreviewService → PreviewSurface →
 * usePreview），不再自行 readFile / Shiki / Markdown / Blob iframe（PREV-002）。
 * 本组件只保留 follow 域自己的状态：narration 状态条（观察终端活动，非文件
 * 预览轮询）与 artifact 卡片。R-E3：只消费共享 UI 原子，不复制预览算法。
 */

export default function FollowRenderer({ filePath }: FollowRendererProps) {
  const locale = useLocale();
  const [narration, setNarration] = useState('');

  // 统一只读预览管线（surface-local controller 由 usePreview 持有）
  const service = useMemo(
    () => new PreviewService(createBuiltinRegistry(), createDefaultContext()),
    [],
  );

  // Narration：观察绑定终端输出以合成「agent 正在做什么」状态条。
  // 这是 follow 域自己的活动状态（非文件预览轮询），终端输出无事件总线，
  // 以低频 1.2s 观察滚动缓冲；文件内容渲染不经此路径。
  useEffect(() => {
    const interval = setInterval(() => {
      const state = getFollowState();
      if (!state.on || !state.currentPath) { setNarration(''); return; }

      // Get terminal output lines for action parsing
      const terminalLines = getScrollbackLines();
      const action = terminalLines.length > 0 ? parseAgentAction(terminalLines) : '';
      const prio = followPriority(state.currentPath);
      const isArtifact = prio === 0;

      // Determine if agent is active (has recent terminal output within 8s)
      const isActive = terminalLines.length > 0 && Date.now() - state.lastActivity < 8000;

      setNarration(composeNarration(
        isActive,
        action,
        state.currentPath,
        isArtifact,
      ));
    }, 1200);
    return () => clearInterval(interval);
  }, []);

  if (!filePath) {
    return (
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        height: '100%', color: 'var(--text-disabled)', fontSize: 'var(--fs-sm)',
      }}>
        {t(locale, 'terminal.followWaiting')}
      </div>
    );
  }

  const prio = followPriority(filePath);

  // Artifacts: show card (build artifacts are not readable text; skip preview)
  if (prio === 0) {
    return (
      <div style={{ padding: SPACING.xl, textAlign: 'center' }}>
        <Package size={32} style={{ color: 'var(--primary)', marginBottom: SPACING.sm }} />
        <div style={{ color: 'var(--text)', fontSize: 'var(--fs-md)', fontWeight: 600 }}>
          {filePath.split('/').pop()}
        </div>
        <div style={{ color: 'var(--text-disabled)', fontSize: FONT_SIZE.sm, marginTop: SPACING.xs }}>
          {t(locale, 'terminal.followArtifact')}
        </div>
        {narration && <NarrationBar text={narration} />}
      </div>
    );
  }

  // 其余类型（HTML/Markdown/Code/Media/…）统一走 Preview Capability 只读管线
  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <PreviewSurface source={{ type: 'file', path: filePath }} surface="follow" service={service} />
      </div>
      {narration && <NarrationBar text={narration} />}
    </div>
  );
}

// ── Narration Bar ──

function NarrationBar({ text }: { text: string }) {
  if (!text) return null;
  return (
    <div style={{
      padding: '4px 12px',
      borderTop: '1px solid var(--border)',
      fontSize: FONT_SIZE.sm,
      color: 'var(--text-secondary)',
      background: 'var(--surface)',
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
    }}>
      {text}
    </div>
  );
}
