'use client';

import { useEffect } from 'react';
import MenubarOverview from './MenubarOverview';

/**
 * MenubarSurface — 菜单栏浮窗入口（route: `?surface=menubar`）。
 *
 * - 加载后立即发 theme_ready_signal（同 ShellLayout 的 FOUC guard 语义：
 *   Popup 以 visible:false 创建，主题就绪后由 Host show()）。
 * - 不挂载 Shell / AssistantWorkspace / Workshop / 更新检查 providers。
 * - 不运行主窗口的全局 hooks。
 */
export default function MenubarSurface() {
  useEffect(() => {
    // CRITICAL: signal theme readiness immediately, before any async work.
    try {
      window.nativesAPI?.themeReady();
    } catch {
      // Browser dev mode — no Tauri window to show.
    }
  }, []);

  return <MenubarOverview />;
}
