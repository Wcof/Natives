'use client';

import { useState, useCallback } from 'react';

export type FollowMode = 'off' | 'terminal-follow' | 'file-follow';

/**
 * 跟随模式 Hook
 * 管理终端与文件浏览器的目录同步
 */
export function useFollowMode() {
  const [mode, setMode] = useState<FollowMode>('off');

  const cycleMode = useCallback(() => {
    setMode((prev) => {
      if (prev === 'off') return 'terminal-follow';
      if (prev === 'terminal-follow') return 'file-follow';
      return 'off';
    });
  }, []);

  /** 终端是否应该跟随文件浏览器 */
  const terminalFollows = mode === 'file-follow';

  /** 文件浏览器是否应该跟随终端 */
  const fileBrowserFollows = mode === 'terminal-follow';

  return {
    mode,
    cycleMode,
    terminalFollows,
    fileBrowserFollows,
    setMode,
  };
}
