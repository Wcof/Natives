'use client';

import { useEffect, useState } from 'react';
import './globals.css';
import '@/lib/tauri-adapter'; // Ensure nativesAPI bridge is injected before any component mounts
import ShellLayout from '@/components/shell/ShellLayout';
import { ThemeProvider } from '@/context/ThemeContext';
import { ToastProvider } from '@/components/ui/Toast';

export default function RootLayout({ children }: { children: React.ReactNode }) {
  const [isWidget, setIsWidget] = useState(false);

  useEffect(() => {
    if (typeof window !== 'undefined' && window.location.search.includes('mode=widget')) {
      setIsWidget(true);
    }

    // Disable default webview context menu in production for native-app feel
    const handleContextMenu = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      // Let custom context menus through (data-custom-context-menu), suppress others
      if (!target.closest('[data-custom-context-menu]') && process.env.NODE_ENV === 'production') {
        e.preventDefault();
      }
    };
    document.addEventListener('contextmenu', handleContextMenu);
    return () => document.removeEventListener('contextmenu', handleContextMenu);
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
            <div className="h-screen w-screen overflow-hidden bg-transparent [&_div[data-sidebar]]:h-screen [&_.vibe-canvas]:h-screen">
              <ShellLayout>{children}</ShellLayout>
            </div>
          </ToastProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
