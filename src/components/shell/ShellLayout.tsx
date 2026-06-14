'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import Sidebar from './Sidebar';
import ContentArea from './ContentArea';
import RightPanel from './RightPanel';
import type { RightPanelMode } from './RightPanel';
import NotificationPanel from './NotificationPanel';
import TerminalPanel from './Terminal';
import CommandPalette from './CommandPalette';
import WorkshopPage from './WorkshopPage';
import SettingsPage from './SettingsPage';
import StorePage from '@/app/store/page';
import FileBrowser from '@/components/files/FileBrowser';
import FilePreview from '@/components/files/FilePreview';
import { type FileEntry } from '@/types/file';
import AiWorkbench from '@/components/ai/AiWorkbench';
import ToolsPage from '@/components/tools/ToolsPage';
import { applyTheme } from '@/lib/theme-engine';
import { getIframeManager } from '@/lib/iframe-manager';
import ScreenshotCard from '@/components/screenshot/ScreenshotCard';
import AnnotationEditor from '@/components/screenshot/AnnotationEditor';
import ReleaseWizardDialog from '@/components/release/ReleaseWizardDialog';
import UpdateNotification from '@/components/update/UpdateNotification';
import '@/types'; // ensure Window.nativesAPI type

interface ShellState {
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  rightPanelMode: RightPanelMode;
  rightPanelWidth: number;
  terminalCollapsed: boolean;
  terminalHeight: number;
  terminalMaximized: boolean;
  cmdkOpen: boolean;
}

export default function ShellLayout({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<ShellState>({
    sidebarCollapsed: false,
    sidebarWidth: 248,
    rightPanelMode: 'closed',
    rightPanelWidth: 320,
    terminalCollapsed: false,
    terminalHeight: 280,
    terminalMaximized: false,
    cmdkOpen: false,
  });

  const [activeView, setActiveView] = useState<string>('dashboard');
  const [themeReady, setThemeReady] = useState(false);
  const [locale, setLocale] = useState<'zh' | 'en'>('zh');
  const terminalSessionIdRef = useRef<string | null>(null);
  const iframeContainerRef = useRef<HTMLDivElement>(null);
  const activeModuleRef = useRef<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  // File preview state
  const [selectedFile, setSelectedFile] = useState<FileEntry | null>(null);

  // Phase 3: Screenshot state
  const [annotatingFile, setAnnotatingFile] = useState<string | null>(null);
  const [annotationImageUrl, setAnnotationImageUrl] = useState<string | null>(null);

  // Phase 3: Release Wizard state
  const [releaseWizardOpen, setReleaseWizardOpen] = useState(false);

  // FOUC guard + locale/theme init
  useEffect(() => {
    setThemeReady(true);

    async function initSettings() {
      try {
        const api = window.nativesAPI;
        if (!api) return;
        const [savedTheme, savedLocale] = await Promise.all([
          api.getTheme(),
          api.getLocale(),
        ]);
        if (savedTheme) applyTheme(savedTheme);
        if (savedLocale) {
          document.documentElement.lang = savedLocale;
          setLocale(savedLocale as 'zh' | 'en');
        }
      } catch {
        applyTheme('terminal-volt');
      }
      try {
        window.nativesAPI?.themeReady();
      } catch {
        // Browser dev mode
      }
    }
    initSettings();
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

  // Focus management: move focus to content area when view changes
  useEffect(() => {
    if (contentRef.current) {
      contentRef.current.focus();
    }
  }, [activeView]);

  // Cmd+B toggle sidebar, Cmd+K command palette, Cmd+Shift+K focus sidebar
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b' && !e.shiftKey) {
        e.preventDefault();
        setState((prev) => ({ ...prev, sidebarCollapsed: !prev.sidebarCollapsed }));
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k' && !e.shiftKey) {
        e.preventDefault();
        setState((prev) => ({ ...prev, cmdkOpen: !prev.cmdkOpen }));
      }
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
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
      // Cmd+N: toggle notifications panel
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'n' && !e.shiftKey) {
        e.preventDefault();
        toggleRightPanel('notifications');
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  // Listen for 'focus-base' messages from iframes (Cmd+Shift+K from iframe context)
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      if (event.data?.type === 'shell:focus-base') {
        const sidebar = document.querySelector<HTMLElement>('[data-sidebar]');
        if (sidebar) {
          const firstFocusable = sidebar.querySelector<HTMLElement>(
            'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
          );
          firstFocusable?.focus();
        }
      }
    };
    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, []);

  // Phase 3: Listen for release wizard and update events from dashboard
  useEffect(() => {
    const handleRelease = () => setReleaseWizardOpen(true);
    const handleUpdate = () => {
      // Trigger update check via dispatch
      window.nativesAPI?.update?.check().catch(() => {});
    };
    // Listen for 'navigate' custom event from DashboardPage/Dashboard
    const handleNavigate = (e: Event) => {
      const view = (e as CustomEvent).detail;
      if (view === '__settings__') setActiveView('settings');
      else if (view === '__workshop__') setActiveView('workshop');
      else if (view === '__store__') setActiveView('store');
      else if (['files', 'ai', 'tools'].includes(view)) setActiveView(view);
    };

    window.addEventListener('open-release-wizard', handleRelease);
    window.addEventListener('check-updates', handleUpdate);
    window.addEventListener('navigate', handleNavigate);
    return () => {
      window.removeEventListener('open-release-wizard', handleRelease);
      window.removeEventListener('check-updates', handleUpdate);
      window.removeEventListener('navigate', handleNavigate);
    };
  }, []);

  const toggleSidebar = useCallback(() => {
    setState((prev) => ({ ...prev, sidebarCollapsed: !prev.sidebarCollapsed }));
  }, []);

  const setRightPanelMode = useCallback((mode: RightPanelMode) => {
    setState((prev) => ({ ...prev, rightPanelMode: mode }));
  }, []);

  const toggleRightPanel = useCallback((mode: RightPanelMode = 'notifications') => {
    setState((prev) => ({
      ...prev,
      rightPanelMode: prev.rightPanelMode === 'closed' ? mode : 'closed',
    }));
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

  const handleInstallModule = useCallback(async (source: string) => {
    try {
      const api = window.nativesAPI;
      if (!api?.module?.install) {
        console.warn('[Shell] Module install API not available');
        return;
      }
      const result = await api.module.install(source);
      if (!result.success && result.error) {
        console.error('[Shell] Module install failed:', result.error);
      }
    } catch (err) {
      console.error('[Shell] Module install error:', err);
    }
  }, []);

  const handleModuleSelect = useCallback((moduleId: string) => {
    if (moduleId === '__settings__') {
      setActiveView('settings');
    } else if (moduleId === '__workshop__') {
      setActiveView('workshop');
    } else if (moduleId === '__store__') {
      setActiveView('store');
    } else if (moduleId === '__notifications__') {
      toggleRightPanel('notifications');
    } else {
      setActiveView(`module:${moduleId}`);
    }
  }, [toggleRightPanel]);

  // File selection handler — opens preview in right panel
  const handleFileSelect = useCallback((entry: FileEntry) => {
    setSelectedFile(entry);
    setRightPanelMode('file-preview');
  }, [setRightPanelMode]);

  const classNames = [
    'shell',
    state.sidebarCollapsed ? 'sidebar-collapsed' : '',
    state.rightPanelMode !== 'closed' ? 'right-panel-open' : '',
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
      case 'files':
        return <FileBrowser onFileSelect={handleFileSelect} />;
      case 'ai':
        return <AiWorkbench />;
      case 'tools':
        return <ToolsPage />;
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
      {/* Skip to content — accessibility */}
      <a href="#main-content" className="skip-to-content">
        {t(locale, 'common.skipToContent')}
      </a>
      <Sidebar
        isCollapsed={state.sidebarCollapsed}
        onToggle={toggleSidebar}
        width={state.sidebarWidth}
        onResize={(w) => setState((prev) => ({ ...prev, sidebarWidth: w }))}
        activeModuleId={activeView}
        onModuleSelect={handleModuleSelect}
        onNotificationClick={() => toggleRightPanel('notifications')}
      />
      <ContentArea>
        <div ref={contentRef} id="main-content" tabIndex={-1} style={{ height: '100%', outline: 'none' }}>
          {renderMainContent()}
        </div>
      </ContentArea>
      <RightPanel
        mode={state.rightPanelMode}
        onModeChange={setRightPanelMode}
        width={state.rightPanelWidth}
        onResize={(w) => setState((prev) => ({ ...prev, rightPanelWidth: w }))}
      >
        {state.rightPanelMode === 'notifications' && (
          <NotificationPanel locale={locale} />
        )}
        {state.rightPanelMode === 'file-preview' && selectedFile && (
          <FilePreview
            entry={selectedFile}
            onClose={() => {
              setSelectedFile(null);
              setRightPanelMode('closed');
            }}
          />
        )}
      </RightPanel>
      <TerminalPanel
        isCollapsed={state.terminalCollapsed}
        onToggle={toggleTerminal}
        height={state.terminalHeight}
        onResize={(h) => setState((prev) => ({ ...prev, terminalHeight: h }))}
        isMaximized={state.terminalMaximized}
        onMaximizeToggle={toggleMaximized}
        onSessionCreated={(id) => { terminalSessionIdRef.current = id; }}
      />
      <CommandPalette
        isOpen={state.cmdkOpen}
        onClose={() => setState((prev) => ({ ...prev, cmdkOpen: false }))}
        onSelect={handleModuleSelect}
        onToggleTerminal={toggleTerminal}
        terminalSessionId={terminalSessionIdRef.current}
      />

      {/* Phase 3: Screenshot Card */}
      <ScreenshotCard
        locale={locale}
        onSendToTerminal={(filePath) => {
          const terminal = document.querySelector<HTMLTextAreaElement>('[data-terminal-input]');
          if (terminal) {
            terminal.value = `!img ${filePath}`;
            terminal.focus();
          }
        }}
        onSaveToMaterial={async (filePath) => {
          try {
            const fs = await import('fs');
            const path = await import('path');
            const desktop = process.env.HOME ? path.join(process.env.HOME, 'Desktop') : '/tmp';
            const materialDir = path.join(desktop, '素材');
            if (!fs.existsSync(materialDir)) fs.mkdirSync(materialDir, { recursive: true });
            const basename = path.basename(filePath);
            fs.copyFileSync(filePath, path.join(materialDir, basename));
          } catch { /* ignore in browser mode */ }
        }}
        onAnnotate={(filePath) => {
          // Load image as data URL for the annotation editor
          setAnnotatingFile(filePath);
          const img = new Image();
          img.onload = () => {
            const canvas = document.createElement('canvas');
            canvas.width = img.naturalWidth;
            canvas.height = img.naturalHeight;
            const ctx = canvas.getContext('2d');
            ctx?.drawImage(img, 0, 0);
            setAnnotationImageUrl(canvas.toDataURL('image/png'));
          };
          img.src = `file://${filePath}`;
        }}
        onDismiss={() => {}}
      />

      {/* Phase 3: Annotation Editor */}
      {annotationImageUrl && annotatingFile && (
        <AnnotationEditor
          locale={locale}
          imageUrl={annotationImageUrl}
          onSave={async (dataUrl) => {
            try {
              const api = window.nativesAPI;
              if (api?.screenshot?.saveAnnotated) {
                const result = await api.screenshot.saveAnnotated(dataUrl, annotatingFile.replace(/\.(png|jpg|jpeg|webp)$/, '-annotated.png'));
                if (result.success) {
                  console.log('[Shell] Annotated image saved:', result.path);
                }
              }
            } catch (err) {
              console.error('[Shell] Failed to save annotation:', err);
            }
            setAnnotatingFile(null);
            setAnnotationImageUrl(null);
          }}
          onClose={() => {
            setAnnotatingFile(null);
            setAnnotationImageUrl(null);
          }}
        />
      )}

      {/* Phase 3: Release Wizard */}
      <ReleaseWizardDialog
        locale={locale}
        isOpen={releaseWizardOpen}
        onClose={() => setReleaseWizardOpen(false)}
      />

      {/* Phase 3: Update Notification */}
      <UpdateNotification locale={locale} />
    </div>
  );
}
