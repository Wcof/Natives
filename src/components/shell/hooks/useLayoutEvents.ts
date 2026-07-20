'use client';

import { useEffect } from 'react';
import type { Locale } from '@/i18n';

interface UseLayoutEventsOptions {
  stateRef: React.RefObject<any>;
  toggleTerminal: () => void;
  setState: (fn: (prev: any) => any) => void;
  setLocale: (locale: Locale) => void;
}

export function useLayoutEvents({
  stateRef,
  toggleTerminal,
  setState,
  setLocale,
}: UseLayoutEventsOptions) {
  // beforeunload persistence (reads stateRef for latest values, never re-binds)
  useEffect(() => {
    const handleBeforeUnload = () => {
      try {
        const api = window.nativesAPI;
        const s = stateRef.current;
        if (api?.db?.set) {
          api.db.set('_state:sidebar', JSON.stringify({
            sidebarWidth: s.sidebarWidth,
            sidebarCollapsed: s.sidebarCollapsed,
            terminalHeight: s.terminalHeight,
            terminalCollapsed: s.terminalCollapsed,
            rightPanelWidth: s.rightPanelWidth,
          }));
        }
      } catch (err) {
        console.warn('[Shell] Failed to save sidebar state:', err);
      }
    };
    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => window.removeEventListener('beforeunload', handleBeforeUnload);
  }, [stateRef]);

  // Locale change listener
  useEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<string>;
      const newLocale = customEvent.detail;
      if (newLocale) {
        document.documentElement.lang = newLocale;
        setLocale(newLocale as Locale);
      } else {
        window.nativesAPI?.getLocale?.().then((saved: string) => {
          if (saved) {
            document.documentElement.lang = saved;
            setLocale(saved as Locale);
          }
        }).catch(() => {});
      }
    };
    window.addEventListener('locale-changed', handler as EventListener);
    return () => window.removeEventListener('locale-changed', handler as EventListener);
  }, [setLocale]);

  // Toggle terminal event
  useEffect(() => {
    const handler = () => toggleTerminal();
    window.addEventListener('toggle-terminal', handler);
    return () => window.removeEventListener('toggle-terminal', handler);
  }, [toggleTerminal]);

  // Open/toggle command palette from UI chrome (e.g. collapsed sidebar search)
  useEffect(() => {
    const openHandler = () => setState((prev: any) => ({ ...prev, cmdkOpen: true }));
    const toggleHandler = () => setState((prev: any) => ({ ...prev, cmdkOpen: !prev.cmdkOpen }));
    window.addEventListener('open-cmdk', openHandler);
    window.addEventListener('toggle-cmdk', toggleHandler);
    return () => {
      window.removeEventListener('open-cmdk', openHandler);
      window.removeEventListener('toggle-cmdk', toggleHandler);
    };
  }, [setState]);

  // Keyboard shortcuts: Cmd+B/K/N
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b' && !e.shiftKey) {
        e.preventDefault();
        setState((prev: any) => ({ ...prev, sidebarCollapsed: !prev.sidebarCollapsed }));
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k' && !e.shiftKey) {
        e.preventDefault();
        setState((prev: any) => ({ ...prev, cmdkOpen: !prev.cmdkOpen }));
      }
      if (e.key === 'Escape') {
        setState((prev: any) => ({ ...prev, cmdkOpen: false }));
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'n' && !e.shiftKey) {
        e.preventDefault();
        const prev = stateRef.current;
        setState({ ...prev, rightPanelMode: prev.rightPanelMode === 'notifications' ? 'closed' : 'notifications' });
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [setState, stateRef]);
}
