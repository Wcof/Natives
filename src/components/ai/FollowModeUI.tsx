'use client';

import { useState, useEffect } from 'react';
import { FollowMode, useFollowMode } from '@/lib/follow-mode';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

export default function FollowModeUI({
  currentDir = '/',
  onNavigate,
}: {
  currentDir?: string;
  onNavigate?: (path: string) => void;
}) {
  const { mode, cycleMode, terminalFollows, fileBrowserFollows } = useFollowMode();
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // 跟随模式切换时触发终端 cd 或文件浏览导航
  useEffect(() => {
    if (mode === 'terminal-follow' && currentDir) {
      // Need a sessionId to write - dispatch event for ShellLayout to handle
      window.dispatchEvent(
        new CustomEvent('follow-cd', { detail: currentDir })
      );
    } else if (mode === 'file-follow' && currentDir) {
      window.dispatchEvent(
        new CustomEvent('navigate-files', { detail: { directory: currentDir } })
      );
    }
  }, [mode, currentDir]);

  const modeLabel: Record<FollowMode, string> = {
    off: t(locale, 'aiWorkbench.followOff'),
    'terminal-follow': t(locale, 'aiWorkbench.followTerminal'),
    'file-follow': t(locale, 'aiWorkbench.followFile'),
  };

  const modeIcon: Record<FollowMode, string> = {
    off: '○',
    'terminal-follow': '⇄',
    'file-follow': '⇄',
  };

  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: SPACING.xs,
      padding: '4px 8px', fontSize: FONT_SIZE.sm,
    }}>
      <button
        className="btn btn-ghost"
        onClick={cycleMode}
        title={modeLabel[mode]}
        style={{
          padding: '2px 8px', fontSize: FONT_SIZE.sm,
          color: mode !== 'off' ? 'var(--accent)' : 'var(--text-dim)',
          border: mode !== 'off' ? '1px solid var(--accent)' : '1px solid var(--border)',
        }}
      >
        {modeIcon[mode]} {mode !== 'off' ? t(locale, 'common.on') : t(locale, 'common.off')}
      </button>

      {terminalFollows && (
        <span style={{ color: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}>
          {t(locale, 'aiWorkbench.follow.cdPrefix')}{currentDir || '/'}
        </span>
      )}

      {fileBrowserFollows && (
        <span style={{ color: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}>
          {t(locale, 'aiWorkbench.followMode.terminalCdSyncsBrowser')}
        </span>
      )}
    </div>
  );
}
