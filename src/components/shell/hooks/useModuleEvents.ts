'use client';

import { useEffect } from 'react';
import { getIframeManager } from '@/lib/iframe-manager';
import { classifyError } from '@/lib/error-classifier';
import { pushRecentModule } from '@/lib/recent-modules';

interface UseModuleEventsOptions {
  activeView: string;
  httpPort: number | null;
  iframeContainerRef: React.RefObject<HTMLDivElement | null>;
  activeModuleRef: React.RefObject<string | null>;
  setCrashedModules: (fn: (prev: Set<string>) => Set<string>) => void;
  setReleaseWizardOpen: (open: boolean) => void;
  setActiveView: (view: string) => void;
}

export function useModuleEvents({
  activeView,
  httpPort,
  iframeContainerRef,
  activeModuleRef,
  setCrashedModules,
  setReleaseWizardOpen,
  setActiveView,
}: UseModuleEventsOptions) {
  // Manage iframe lifecycle when activeView changes
  useEffect(() => {
    const container = iframeContainerRef.current;
    if (!container) return;

    const manager = getIframeManager();
    const moduleId = activeView.startsWith('module:') ? activeView.slice(7) : null;

    if (activeModuleRef.current && activeModuleRef.current !== moduleId) {
      manager.hideIframe(activeModuleRef.current);
    }

    activeModuleRef.current = moduleId;

    if (moduleId) {
      pushRecentModule(moduleId);
    }

    if (!moduleId) {
      const existingIframes = container.querySelectorAll('iframe');
      existingIframes.forEach((f) => f.remove());
      return;
    }

    let iframe = manager?.showIframe(moduleId);
    if (!iframe) {
      const url = `http://localhost:${httpPort}/modules/${moduleId}/index.html`;
      iframe = manager?.createIframe(moduleId, url);

      manager?.startHeartbeat(moduleId, 5000);
      manager?.onHeartbeatTimeout(moduleId, () => {
        console.warn(`[Shell] Heartbeat timeout for module ${moduleId}`);
      });
      manager?.onCrash(moduleId, () => {
        console.error(`[Shell] Module ${moduleId} crashed`);
        setCrashedModules((prev) => {
          if (!prev.has(moduleId)) {
            try {
              const classified = classifyError(new Error(`Plugin ${moduleId} crashed: Heartbeat timeout`), moduleId);
              window.nativesAPI?.notification?.send?.(
                `Plugin ${moduleId} crashed`,
                `Heartbeat timeout — ${classified.actionHint}`,
                'error',
              );
            } catch { /* notification persistence is best-effort */ }
          }
          const next = new Set(prev);
          next.add(moduleId);
          return next;
        });
      });
    }

    container.innerHTML = '';
    container.appendChild(iframe);
    iframe.style.width = '100%';
    iframe.style.height = '100%';
    iframe.style.border = 'none';

    const handleMessage = (event: MessageEvent) => {
      if (event.data?.type === 'lifecycle:heartbeat' && event.data?.moduleId === moduleId) {
        manager?.onHeartbeatReceived(moduleId);
      }
    };
    window.addEventListener('message', handleMessage);
    return () => {
      window.removeEventListener('message', handleMessage);
    };
  }, [activeView, httpPort, iframeContainerRef, activeModuleRef, setCrashedModules]);

  // Release wizard, update, and navigate events
  useEffect(() => {
    const handleRelease = () => setReleaseWizardOpen(true);
    const handleNavigate = (e: Event) => {
      const view = (e as CustomEvent).detail;
      if (view === '__settings__') setActiveView('settings');
      else if (view === '__workshop__') setActiveView('workshop');
      else if (view === 'ai') setActiveView('ai');
      else if (view === 'tools') setActiveView('tools');
      else if (view === 'modules') setActiveView('modules');
      else if (typeof view === 'string' && view.startsWith('files')) {
        setActiveView('files');
        const qIndex = view.indexOf('?');
        if (qIndex >= 0) {
          const params = new URLSearchParams(view.slice(qIndex + 1));
          const navPath = params.get('path');
          if (navPath) {
            (window as any).__pendingNavigateFiles = navPath;
            window.dispatchEvent(new CustomEvent('navigate-files', { detail: navPath }));
          }
        }
      }
    };

    window.addEventListener('open-release-wizard', handleRelease);
    window.addEventListener('navigate', handleNavigate);
    return () => {
      window.removeEventListener('open-release-wizard', handleRelease);
      window.removeEventListener('navigate', handleNavigate);
    };
  }, [setReleaseWizardOpen, setActiveView]);

  // File flash animation via IPC
  useEffect(() => {
    const handler = (_event: unknown, channel: string, data: unknown) => {
      if (channel === 'file:changed') {
        const filePath = typeof data === 'string' ? data : (data as { path?: string })?.path || '';
        if (filePath) {
          window.dispatchEvent(new CustomEvent('file-flash', { detail: filePath }));
        }
      }
    };
    const api = window.nativesAPI;
    if (api?.onDbStateChanged) {
      const unsub = api.onDbStateChanged(handler);
      return () => { unsub?.(); };
    }
  }, []);

  // Long task complete → system notification
  useEffect(() => {
    if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
      Notification.requestPermission().catch(() => {});
    }
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      const title = detail?.title || 'Task Complete';
      const body = detail?.message || 'A long-running task has completed.';
      if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
        new Notification(title, { body });
      }
    };
    window.addEventListener('long-task-complete', handler);
    return () => window.removeEventListener('long-task-complete', handler);
  }, []);
}
