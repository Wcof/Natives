'use client';

import { useEffect, type ReactNode } from 'react';
import { useHydrated } from '@/hooks/useHydrated';
import '@/lib/tauri-adapter';
import ShellLayout from '@/components/shell/ShellLayout';
import { ThemeProvider } from '@/context/ThemeContext';
import { ToastProvider } from '@/components/ui/Toast';

/* ═══════════════════════════════════════════════
   RootClient — Client Component
   Hydration, interactivity, providers.
   (Does NOT import global CSS — that lives in layout.tsx)
   ═══════════════════════════════════════════════ */

export default function RootClient({ children }: { children: ReactNode }) {
  const isWidget = useHydrated() && typeof window !== 'undefined' && window.location.search.includes('mode=widget');

  useEffect(() => {
    if (isWidget) {
      document.documentElement.classList.add('vibe-widget-mode');
    }

    const handleContextMenu = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest('[data-custom-context-menu]') && process.env.NODE_ENV === 'production') {
        e.preventDefault();
      }
    };
    document.addEventListener('contextmenu', handleContextMenu);
    return () => document.removeEventListener('contextmenu', handleContextMenu);
  }, []);

  return (
    <ThemeProvider>
      <ToastProvider>
        <div className="h-full w-full overflow-hidden bg-transparent [&_div[data-sidebar]]:h-full [&_.vibe-canvas]:h-full">
          <ShellLayout>{children}</ShellLayout>
        </div>
      </ToastProvider>
    </ThemeProvider>
  );
}
