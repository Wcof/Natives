'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import Sidebar from './Sidebar';
import ContentArea from './ContentArea';
import RightPanel from './RightPanel';
import TerminalPanel from './Terminal';
import CommandPalette from './CommandPalette';
import WorkshopPage from './WorkshopPage';
import SettingsPage from './SettingsPage';
import StorePage from '@/app/store/page';
import { applyTheme } from '@/lib/theme-engine';
import { getIframeManager } from '@/lib/iframe-manager';
import '@/types'; // ensure Window.nativesAPI type

interface ShellState {
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  rightPanelOpen: boolean;
  rightPanelTitle: string;
  terminalCollapsed: boolean;
  terminalHeight: number;
  terminalMaximized: boolean;
  cmdkOpen: boolean;
}

export default function ShellLayout({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<ShellState>({
    sidebarCollapsed: false,
    sidebarWidth: 248,
    rightPanelOpen: false,
    rightPanelTitle: 'Panel',
    terminalCollapsed: false,
    terminalHeight: 280,
    terminalMaximized: false,
    cmdkOpen: false,
  });

  const [activeView, setActiveView] = useState<string>('dashboard');
  const [themeReady, setThemeReady] = useState(false);
  const iframeContainerRef = useRef<HTMLDivElement>(null);
  const activeModuleRef = useRef<string | null>(null);

  // FOUC guard
  useEffect(() => {
    setThemeReady(true);
    applyTheme('terminal-volt');

    // Notify Electron main process via IPC
    try {
      if (window.nativesAPI?.themeReady) {
        window.nativesAPI.themeReady();
      }
    } catch {
      // Browser dev mode
    }
  }, []);

  // Manage iframe lifecycle when activeView changes
  useEffect(() => {
    const container = iframeContainerRef.current;
    if (!container) return;

    const manager = getIframeManager();
    const moduleId = activeView.startsWith('module:') ? activeView.slice(7) : null;

    // Hide previous iframe
    if (activeModuleRef.current && activeModuleRef.current !== moduleId) {
      manager.hideIframe(activeModuleRef.current);
    }

    activeModuleRef.current = moduleId;

    if (!moduleId) {
      // Remove any existing iframes from container (for non-module views)
      const existingIframes = container.querySelectorAll('iframe');
      existingIframes.forEach((f) => f.remove());
      return;
    }

    // Check if iframe already exists
    let iframe = manager.showIframe(moduleId);
    if (!iframe) {
      // Create new iframe
      const port = window.__nativesHttpPort || 3000;
      const url = `http://localhost:${port}/modules/${moduleId}/index.html`;
      iframe = manager.createIframe(moduleId, url);

      // Set up heartbeat monitoring
      manager.startHeartbeat(moduleId, 5000);
      manager.onHeartbeatTimeout(moduleId, () => {
        console.warn(`[Shell] Heartbeat timeout for module ${moduleId}`);
      });
      manager.onCrash(moduleId, () => {
        console.error(`[Shell] Module ${moduleId} crashed`);
      });
    }

    // Move iframe into container
    container.innerHTML = '';
    container.appendChild(iframe);
    iframe.style.width = '100%';
    iframe.style.height = '100%';
    iframe.style.border = 'none';

    // Listen for heartbeat messages from the iframe
    const handleMessage = (event: MessageEvent) => {
      if (event.data?.type === 'lifecycle:heartbeat' && event.data?.moduleId === moduleId) {
        manager.onHeartbeatReceived(moduleId);
      }
    };
    window.addEventListener('message', handleMessage);

    return () => {
      window.removeEventListener('message', handleMessage);
    };
  }, [activeView]);

  // Cmd+B toggle sidebar, Cmd+K command palette, Cmd+Shift+K focus sidebar
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'b') {
        e.preventDefault();
        setState((prev) => ({ ...prev, sidebarCollapsed: !prev.sidebarCollapsed }));
      }
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setState((prev) => ({ ...prev, cmdkOpen: !prev.cmdkOpen }));
      }
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key === 'K') {
        e.preventDefault();
        // Blur current active element, focus sidebar's first focusable
        if (document.activeElement instanceof HTMLElement) {
          document.activeElement.blur();
        }
        const sidebar = document.querySelector<HTMLElement>('[data-sidebar]');
        if (sidebar) {
          const firstFocusable = sidebar.querySelector<HTMLElement>(
            'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
          );
          firstFocusable?.focus();
        }
      }
      if (e.key === 'Escape') {
        setState((prev) => ({ ...prev, cmdkOpen: false }));
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  const toggleSidebar = useCallback(() => {
    setState((prev) => ({ ...prev, sidebarCollapsed: !prev.sidebarCollapsed }));
  }, []);

  const toggleRightPanel = useCallback((title = 'Panel') => {
    setState((prev) => ({ ...prev, rightPanelOpen: !prev.rightPanelOpen, rightPanelTitle: title }));
  }, []);

  const toggleTerminal = useCallback(() => {
    setState((prev) => ({ ...prev, terminalCollapsed: !prev.terminalCollapsed }));
  }, []);

  // Listen for 'toggle-terminal' custom event (dispatched by CommandPalette)
  useEffect(() => {
    const handler = () => toggleTerminal();
    window.addEventListener('toggle-terminal', handler);
    return () => window.removeEventListener('toggle-terminal', handler);
  }, [toggleTerminal]);

  const toggleMaximized = useCallback(() => {
    setState((prev) => ({ ...prev, terminalMaximized: !prev.terminalMaximized }));
  }, []);

  const openCmdk = useCallback(() => {
    setState((prev) => ({ ...prev, cmdkOpen: true }));
  }, []);

  const handleInstallModule = useCallback((source: string) => {
    window.nativesAPI?.module?.install?.(source);
  }, []);

  const handleModuleSelect = useCallback((moduleId: string) => {
    if (moduleId === '__settings__') {
      setActiveView('settings');
    } else if (moduleId === '__workshop__') {
      setActiveView('workshop');
    } else if (moduleId === '__store__') {
      setActiveView('store');
    } else {
      setActiveView(`module:${moduleId}`);
    }
  }, []);

  const classNames = [
    'shell',
    state.sidebarCollapsed ? 'sidebar-collapsed' : '',
    state.rightPanelOpen ? 'right-panel-open' : '',
    state.terminalCollapsed ? 'terminal-collapsed' : '',
    state.terminalMaximized ? 'terminal-maximized' : '',
  ]
    .filter(Boolean)
    .join(' ');

  const renderMainContent = () => {
    switch (activeView) {
      case 'settings':
        return <SettingsPage />;
      case 'workshop':
        return <WorkshopPage onInstall={handleInstallModule} />;
      case 'store':
        return <StorePage />;
      case 'dashboard':
        return children;
      default:
        if (activeView.startsWith('module:')) {
          // iframe is managed by useEffect above, render container
          return <div ref={iframeContainerRef} style={{ width: '100%', height: '100%' }} />;
        }
        return children;
    }
  };

  return (
    <div className={classNames} style={{ opacity: themeReady ? 1 : 0 }}>
      <Sidebar
        isCollapsed={state.sidebarCollapsed}
        onToggle={toggleSidebar}
        width={state.sidebarWidth}
        onResize={(w) => setState((prev) => ({ ...prev, sidebarWidth: w }))}
        activeModuleId={activeView}
        onModuleSelect={handleModuleSelect}
      />
      <ContentArea>
        {renderMainContent()}
      </ContentArea>
      <RightPanel
        isOpen={state.rightPanelOpen}
        onToggle={() => toggleRightPanel()}
        title={state.rightPanelTitle}
      />
      <TerminalPanel
        isCollapsed={state.terminalCollapsed}
        onToggle={toggleTerminal}
        height={state.terminalHeight}
        onResize={(h) => setState((prev) => ({ ...prev, terminalHeight: h }))}
        isMaximized={state.terminalMaximized}
        onMaximizeToggle={toggleMaximized}
      />
      <CommandPalette
        isOpen={state.cmdkOpen}
        onClose={() => setState((prev) => ({ ...prev, cmdkOpen: false }))}
        onSelect={handleModuleSelect}
      />
    </div>
  );
}
