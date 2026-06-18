'use client';

import { useEffect, useState, useCallback } from 'react';
import './globals.css';
import ShellLayout from '@/components/shell/ShellLayout';
import { ThemeProvider } from '@/context/ThemeContext';
import { ToastProvider } from '@/components/ui/Toast';

/** macOS 风格交通灯按钮颜色 */
const TRAFFIC = [
  { key: 'close',    color: '#ff5f57', hover: '#ff3b30', label: '关闭' },
  { key: 'minimize', color: '#febc2e', hover: '#f5a623', label: '最小化' },
  { key: 'maximize', color: '#28c840', hover: '#1fa836', label: '最大化' },
] as const;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  const [isWidget, setIsWidget] = useState(false);
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (typeof window !== 'undefined' && window.location.search.includes('mode=widget')) {
      setIsWidget(true);
    }
  }, []);

  // 轮询窗口最大化状态（最大化按钮图标切换）
  useEffect(() => {
    if (isWidget) return;
    const poll = setInterval(async () => {
      try {
        const m = await window.nativesAPI?.windowControls?.isMaximized?.();
        if (m !== undefined) setIsMaximized(m);
      } catch { /* ignore */ }
    }, 300);
    return () => clearInterval(poll);
  }, [isWidget]);

  const handleTraffic = useCallback(async (action: 'minimize' | 'maximize' | 'close') => {
    const ctrl = window.nativesAPI?.windowControls;
    if (!ctrl) return;
    if (action === 'minimize') await ctrl.minimize();
    else if (action === 'close') await ctrl.close();
    else {
      await ctrl.maximize();
      // 主动刷新本地状态
      try { setIsMaximized(await ctrl.isMaximized()); } catch { /* ignore */ }
    }
  }, []);

  return (
    <html
      lang="zh-CN"
      data-theme="terminal-volt"
      className={`h-full bg-transparent ${isWidget ? 'vibe-widget-mode' : ''}`}
    >
      <body className="h-full overflow-hidden bg-transparent text-content-text antialiased">
        <ThemeProvider>
          <ToastProvider>
            <div className="h-screen w-screen overflow-hidden bg-transparent [--electron-titlebar-safe-area:env(titlebar-area-height,38px)] pt-[var(--electron-titlebar-safe-area)] [&_div[data-sidebar]]:h-[calc(100vh_-_var(--electron-titlebar-safe-area))] [&_.vibe-canvas]:h-[calc(100vh_-_var(--electron-titlebar-safe-area))]">
              {/* ── 自定义标题栏（frame:false 拖拽区 + macOS 交通灯按钮）── */}
              {!isWidget && (
                <div
                  className="fixed top-0 left-0 right-0 h-[var(--electron-titlebar-safe-area,38px)] z-50 select-none"
                  style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}
                >
                  {/* 交通灯按钮组 — 左上角 */}
                  <div
                    className="absolute left-3 top-1/2 -translate-y-1/2 flex items-center gap-[8px]"
                    style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
                  >
                    {TRAFFIC.map(({ key, color, hover, label }) => (
                      <button
                        key={key}
                        onClick={() => handleTraffic(key as 'minimize' | 'maximize' | 'close')}
                        aria-label={label}
                        title={label}
                        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.backgroundColor = hover; }}
                        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.backgroundColor = color; }}
                        style={{
                          width: 12,
                          height: 12,
                          borderRadius: '50%',
                          backgroundColor: color,
                          border: 'none',
                          cursor: 'pointer',
                          padding: 0,
                          transition: 'background-color 0.1s ease',
                          // 最大化按钮在窗口已最大化时显示还原图标
                          position: 'relative',
                        }}
                      >
                        {/* 最大化/还原指示：窗口最大化时在绿点上显示 ─ 符号 */}
                        {key === 'maximize' && isMaximized && (
                          <span
                            style={{
                              position: 'absolute',
                              inset: 0,
                              display: 'flex',
                              alignItems: 'center',
                              justifyContent: 'center',
                              fontSize: 8,
                              fontWeight: 700,
                              color: '#1a6b1a',
                              lineHeight: 1,
                            }}
                          >
                            ─
                          </span>
                        )}
                      </button>
                    ))}
                  </div>
                </div>
              )}
              <ShellLayout>{children}</ShellLayout>
            </div>
          </ToastProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
