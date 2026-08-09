'use client';

import { useEffect, useRef } from 'react';
import type { Locale } from '@/i18n';
import { FILE_EVENTS, onFileEvent } from '@/lib/file-events';
import type { ShellState } from '../useShellState';

export interface LayoutPersistSnapshot {
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  terminalHeight: number;
  terminalCollapsed: boolean;
  rightPanelWidth: number;
}

interface UseLayoutEventsOptions {
  stateRef: React.RefObject<ShellState>;
  /** Layout fields that should be debounced to `_state:sidebar`. */
  layoutPersist: LayoutPersistSnapshot;
  toggleTerminal: () => void;
  setState: React.Dispatch<React.SetStateAction<ShellState>>;
  setLocale: (locale: Locale) => void;
}

const LAYOUT_PERSIST_DEBOUNCE_MS = 350;

function persistLayoutState(snapshot: LayoutPersistSnapshot | null | undefined) {
  try {
    const api = window.nativesAPI;
    if (!api?.db?.set || !snapshot) return;
    api.db.set('_state:sidebar', JSON.stringify({
      sidebarWidth: snapshot.sidebarWidth,
      sidebarCollapsed: snapshot.sidebarCollapsed,
      terminalHeight: snapshot.terminalHeight,
      terminalCollapsed: snapshot.terminalCollapsed,
      rightPanelWidth: snapshot.rightPanelWidth,
    }));
  } catch (err) {
    console.warn('[Shell] Failed to save sidebar state:', err);
  }
}

export function useLayoutEvents({
  stateRef,
  layoutPersist,
  toggleTerminal,
  setState,
  setLocale,
}: UseLayoutEventsOptions) {
  const persistTimerRef = useRef<number | null>(null);
  const skipFirstPersistRef = useRef(true);

  // Debounced persist when the user drags / toggles layout chrome.
  // Skip the initial mount so we don't overwrite DB with defaults before restore.
  useEffect(() => {
    if (skipFirstPersistRef.current) {
      skipFirstPersistRef.current = false;
      return;
    }
    if (persistTimerRef.current) window.clearTimeout(persistTimerRef.current);
    persistTimerRef.current = window.setTimeout(() => {
      persistTimerRef.current = null;
      persistLayoutState(layoutPersist);
    }, LAYOUT_PERSIST_DEBOUNCE_MS);
    return () => {
      if (persistTimerRef.current) {
        window.clearTimeout(persistTimerRef.current);
        persistTimerRef.current = null;
      }
    };
  }, [
    layoutPersist.sidebarWidth,
    layoutPersist.sidebarCollapsed,
    layoutPersist.terminalHeight,
    layoutPersist.terminalCollapsed,
    layoutPersist.rightPanelWidth,
  ]);

  // Final flush on unload (covers kill-before-debounce-fires).
  useEffect(() => {
    const handleBeforeUnload = () => {
      if (persistTimerRef.current) {
        window.clearTimeout(persistTimerRef.current);
        persistTimerRef.current = null;
      }
      const s = stateRef.current;
      if (s) {
        persistLayoutState({
          sidebarWidth: s.sidebarWidth,
          sidebarCollapsed: s.sidebarCollapsed,
          terminalHeight: s.terminalHeight,
          terminalCollapsed: s.terminalCollapsed,
          rightPanelWidth: s.rightPanelWidth,
        });
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

  // 展开终端事件（幂等：仅在折叠时展开，绝不关闭已打开面板；file-events 契约）
  useEffect(
    () => onFileEvent(FILE_EVENTS.openTerminal, () => {
      setState((prev) => (prev.terminalCollapsed ? { ...prev, terminalCollapsed: false } : prev));
    }),
    [setState],
  );

  // Open/toggle command palette from UI chrome (e.g. collapsed sidebar search)
  useEffect(() => {
    const openHandler = () => setState((prev) => ({ ...prev, cmdkOpen: true }));
    const toggleHandler = () => setState((prev) => ({ ...prev, cmdkOpen: !prev.cmdkOpen }));
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
        setState((prev) => ({ ...prev, sidebarCollapsed: !prev.sidebarCollapsed }));
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k' && !e.shiftKey) {
        // 终端有焦点时让位给终端的 Cmd+K 清屏（否则双重触发）
        const target = e.target as HTMLElement | null;
        if (target?.closest?.('.terminal-panel')) return;
        e.preventDefault();
        setState((prev) => ({ ...prev, cmdkOpen: !prev.cmdkOpen }));
      }
      if (e.key === 'Escape') {
        // 只在面板开着时发 setState，避免每次 Esc 都发一次空更新并
        // 与其他 Esc 消费者（模态等）抢事件
        setState((prev) => (prev.cmdkOpen ? { ...prev, cmdkOpen: false } : prev));
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
