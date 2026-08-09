'use client';

import { lazy, Suspense, useEffect, type ReactNode } from 'react';
import { useHydrated } from '@/hooks/useHydrated';
import '@/lib/tauri-adapter';
const LazyShellLayout = lazy(() => import('@/components/shell/ShellLayout'));
const LazyMenubarSurface = lazy(() => import('@/components/menubar/MenubarSurface'));
import { ThemeProvider } from '@/context/ThemeContext';
import { ToastProvider } from '@/components/ui/Toast';
import { AssistantWorkspaceProvider } from '@/components/assistant/AssistantWorkspaceContext';

/* ═══════════════════════════════════════════════
   RootClient — Client Component
   Hydration, interactivity, providers.
   (Does NOT import global CSS — that lives in layout.tsx)

   Surface routing happens HERE, at the earliest split (MB-P0-03):
   - `?surface=menubar` → lightweight MenubarSurface. It deliberately does NOT
     mount Shell, AssistantWorkspace, Workshop, update checks or main-window
     global hooks — only the minimal theme context needed for the FOUC guard.
   - everything else → full ShellLayout (unchanged).
   ═══════════════════════════════════════════════ */

export default function RootClient({ children }: { children: ReactNode }) {
  const hydrated = useHydrated();
  const isMenubar =
    hydrated && typeof window !== 'undefined' && window.location.search.includes('surface=menubar');
  const isWidget =
    hydrated && typeof window !== 'undefined' && window.location.search.includes('mode=widget');

  useEffect(() => {
    if (isMenubar) {
      document.documentElement.classList.add('menubar-mode');
    } else if (isWidget) {
      document.documentElement.classList.add('widget-mode');
    }

    const handleContextMenu = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest('[data-custom-context-menu]') && process.env.NODE_ENV === 'production') {
        e.preventDefault();
      }
    };
    document.addEventListener('contextmenu', handleContextMenu);
    return () => document.removeEventListener('contextmenu', handleContextMenu);
  }, [isMenubar, isWidget]);

  // Earliest split: the menubar popup is a self-contained lightweight surface.
  // It must never load the Shell / Assistant / Workshop bundle or their hooks.
  if (isMenubar) {
    return (
      <ThemeProvider>
        <Suspense fallback={null}>
          <LazyMenubarSurface />
        </Suspense>
      </ThemeProvider>
    );
  }

  return (
    <ThemeProvider>
      <ToastProvider>
        <AssistantWorkspaceProvider>
          <div className="h-full w-full overflow-hidden bg-transparent [&_div[data-sidebar]]:h-full [&_[data-shell-content]]:h-full">
            <Suspense fallback={null}><LazyShellLayout>{children}</LazyShellLayout></Suspense>
          </div>
        </AssistantWorkspaceProvider>
      </ToastProvider>
    </ThemeProvider>
  );
}
