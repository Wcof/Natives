'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import Sidebar from './Sidebar';
import ContentArea from './ContentArea';
import RightPanel from './RightPanel';
import TerminalPanel from './Terminal';
import CommandPalette from './CommandPalette';
import WorkshopPage from './WorkshopPage';
import SettingsPage from './SettingsPage';
import { applyTheme, onThemeChange } from '@/lib/theme-engine';

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

  // FOUC guard
  useEffect(() => {
    setThemeReady(true);
    applyTheme('terminal-volt');

    // Notify Electron main process
    try {
      const event = new CustomEvent('theme-applied-ready');
      window.dispatchEvent(event);
    } catch {
      // Browser dev mode
    }
  }, []);

  // Cmd+B toggle sidebar, Cmd+K command palette
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

  const toggleMaximized = useCallback(() => {
    setState((prev) => ({ ...prev, terminalMaximized: !prev.terminalMaximized }));
  }, []);

  const openCmdk = useCallback(() => {
    setState((prev) => ({ ...prev, cmdkOpen: true }));
  }, []);

  const handleModuleSelect = useCallback((moduleId: string) => {
    if (moduleId === '__settings__') {
      setActiveView('settings');
    } else if (moduleId === '__workshop__') {
      setActiveView('workshop');
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
        return <WorkshopPage onInstall={() => {}} />;
      case 'dashboard':
        return children;
      default:
        if (activeView.startsWith('module:')) {
          const moduleId = activeView.slice(7);
          return (
            <iframe
              sandbox="allow-scripts allow-forms"
              src={`/modules/${moduleId}/index.html`}
              style={{ width: '100%', height: '100%', border: 'none' }}
              title={moduleId}
            />
          );
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
