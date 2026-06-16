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
import ErrorBoundary from '@/components/ui/ErrorBoundary';
import ShortcutHelp from '@/components/ui/ShortcutHelp';
import WorkshopPage from './WorkshopPage';
import SettingsPage from './SettingsPage';
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
import UsernameOnboarding from '@/components/onboarding/UsernameOnboarding';
import { classifyError } from '@/lib/error-classifier';
import { useFollowMode } from '@/lib/follow-mode';
import { pushRecentModule } from '@/lib/recent-modules';
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
  const [locale, setLocale] = useState<Locale>('zh');
  const terminalSessionIdRef = useRef<string | null>(null);
  const iframeContainerRef = useRef<HTMLDivElement>(null);
  const activeModuleRef = useRef<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  // File preview state
  const [selectedFile, setSelectedFile] = useState<FileEntry | null>(null);

  // Terminal follow mode — unified via useFollowMode hook
  const { mode: followMode, cycleMode: cycleFollowMode } = useFollowMode();

  // Crash state: track crashed modules for overlay display
  const [crashedModules, setCrashedModules] = useState<Set<string>>(new Set());
  const [iframeReloadKey, setIframeReloadKey] = useState(0);

  // P1-5: Username onboarding
  const [needsOnboarding, setNeedsOnboarding] = useState<boolean | null>(null);

  // Phase 3: Screenshot state
  const [annotatingFile, setAnnotatingFile] = useState<string | null>(null);
  const [annotationImageUrl, setAnnotationImageUrl] = useState<string | null>(null);

  // Phase 3: Release Wizard state
  const [releaseWizardOpen, setReleaseWizardOpen] = useState(false);

  // FOUC guard + locale/theme init + state persistence load
  useEffect(() => {
    setThemeReady(true);

    async function initSettings() {
      const api = window.nativesAPI;
      if (!api) return;

      try {
        const savedTheme = await api.getTheme();
        if (savedTheme) applyTheme(savedTheme);
      } catch (err) {
        console.error('[Shell] Failed to load saved theme:', err);
        applyTheme('editorial');
      }

      try {
        const savedLocale = await api.getLocale();
        if (savedLocale) {
          document.documentElement.lang = savedLocale;
          setLocale(savedLocale as Locale);
        }
      } catch (err) {
        console.error('[Shell] Failed to load saved locale:', err);
      }

      // Restore persisted sidebar state
      try {
        const api = window.nativesAPI;
        if (api?.db?.get) {
          const savedStr = await api.db.get('_state:sidebar');
          if (savedStr) {
            const saved = JSON.parse(savedStr as string);
            if (saved) {
              setState((prev) => ({
                ...prev,
                ...(typeof saved.sidebarWidth === 'number' && { sidebarWidth: saved.sidebarWidth }),
                ...(typeof saved.sidebarCollapsed === 'boolean' && { sidebarCollapsed: saved.sidebarCollapsed }),
                ...(typeof saved.terminalHeight === 'number' && { terminalHeight: saved.terminalHeight }),
                ...(typeof saved.terminalCollapsed === 'boolean' && { terminalCollapsed: saved.terminalCollapsed }),
                ...(typeof saved.rightPanelWidth === 'number' && { rightPanelWidth: saved.rightPanelWidth }),
              }));
            }
          }
        }
      } catch (err) {
        console.warn('[Shell] Failed to load sidebar state:', err);
      }

      try {
        window.nativesAPI?.themeReady();
      } catch {
        // Browser dev mode
      }
    }
    initSettings();

    // Persist sidebar state on page unload
    const handleBeforeUnload = () => {
      try {
        const api = window.nativesAPI;
        if (api?.db?.set) {
          api.db.set('_state:sidebar', JSON.stringify({
            sidebarWidth: state.sidebarWidth,
            sidebarCollapsed: state.sidebarCollapsed,
            terminalHeight: state.terminalHeight,
            terminalCollapsed: state.terminalCollapsed,
            rightPanelWidth: state.rightPanelWidth,
          }));
        }
      } catch (err) {
        console.warn('[Shell] Failed to save sidebar state:', err);
      }
    };
    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload);
    };
  }, [state.sidebarWidth, state.sidebarCollapsed, state.terminalHeight, state.terminalCollapsed, state.rightPanelWidth]);

  // Reactively update locale when SettingsPage changes language
  useEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<string>;
      const newLocale = customEvent.detail;
      if (newLocale) {
        document.documentElement.lang = newLocale;
        setLocale(newLocale as Locale);
      } else {
        window.nativesAPI?.getLocale?.().then((saved) => {
          if (saved) {
            document.documentElement.lang = saved;
            setLocale(saved as Locale);
          }
        }).catch(() => {});
      }
    };
    window.addEventListener('locale-changed', handler as EventListener);
    return () => window.removeEventListener('locale-changed', handler as EventListener);
  }, []);

  // P1-5: Check if username is set (first-run detection)
  useEffect(() => {
    const api = window.nativesAPI;
    if (!api?.db?.get) {
      // No native API (browser dev mode) — skip onboarding
      setNeedsOnboarding(false);
      return;
    }
    api.db.get('settings:username').then((value: unknown) => {
      setNeedsOnboarding(!value);
    }).catch(() => setNeedsOnboarding(false));
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

    // Record this open in the LRU "recently used" list (US1 Dashboard). See
    // ISSUE-3. Only meaningful for actual module views.
    if (moduleId) {
      pushRecentModule(moduleId);
    }

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
        setCrashedModules((prev) => {
          // Persist the crash to the notifications table so it shows up in
          // the notification center (US20). Only fire once per crash to avoid
          // duplicate rows when the callback fires repeatedly. See BUG-3.
          if (!prev.has(moduleId)) {
            try {
              // P1-6: Use classifyError to enrich crash notification with actionHint
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

  // Terminal follow mode: send cd to active terminal when file browser navigates
  useEffect(() => {
    if (followMode !== 'terminal-follow') return;
    const handler = (e: Event) => {
      const dirPath = (e as CustomEvent).detail;
      if (typeof dirPath !== 'string' || !dirPath.startsWith('/')) return;
      const sessionId = terminalSessionIdRef.current;
      if (!sessionId) return;
      const api = window.nativesAPI;
      if (api?.terminal?.write) {
        // Send cd command with proper quoting
        api.terminal.write(sessionId, `cd "${dirPath}"\r`);
      }
    };
    window.addEventListener('navigate-files', handler);
    return () => window.removeEventListener('navigate-files', handler);
  }, [followMode]);

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
      else if (view === '__store__') setActiveView('workshop');
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

  // File flash animation: listen for file write events via IPC
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

  // System notification support: listen for long-task-complete events
  useEffect(() => {
    // Request notification permission on mount
    if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
      Notification.requestPermission().catch(() => {});
    }

    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      const title = detail?.title || t(locale, 'common.notificationTitle');
      const body = detail?.message || t(locale, 'common.notificationBody');
      if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
        new Notification(title, { body });
      }
    };
    window.addEventListener('long-task-complete', handler);
    return () => window.removeEventListener('long-task-complete', handler);
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
      setActiveView('workshop');
    } else if (moduleId === '__notifications__') {
      toggleRightPanel('notifications');
    } else if (moduleId.startsWith('__files__:')) {
      // Navigate file browser to a specific path
      const path = moduleId.slice(10);
      setActiveView('files');
      // Dispatch event for FileBrowser to pick up
      window.dispatchEvent(new CustomEvent('navigate-files', { detail: path }));
    } else {
      setActiveView(`module:${moduleId}`);
      setRightPanelMode('module-details');
    }
  }, [toggleRightPanel, setRightPanelMode]);

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

  // P1-5: Show onboarding if no username is set
  if (needsOnboarding === null) return null;
  if (needsOnboarding) {
    return <UsernameOnboarding locale={locale} onComplete={() => setNeedsOnboarding(false)} />;
  }

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
        locale={locale}
      />
      <ContentArea>
        <div ref={contentRef} id="main-content" tabIndex={-1} style={{ height: '100%', outline: 'none' }}>
          <ErrorBoundary>
            {renderMainContent()}
          </ErrorBoundary>
        </div>
      </ContentArea>
      <RightPanel
        mode={state.rightPanelMode}
        onModeChange={setRightPanelMode}
        width={state.rightPanelWidth}
        onResize={(w) => setState((prev) => ({ ...prev, rightPanelWidth: w }))}
        title={state.rightPanelMode === 'file-preview' && selectedFile ? selectedFile.name : undefined}
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
        {state.rightPanelMode === 'module-details' && activeView.startsWith('module:') && (
          <ModuleDetails moduleId={activeView.slice(7)} locale={locale} />
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
        followMode={followMode !== 'off'}
        onFollowModeToggle={cycleFollowMode}
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
            // Copy file to Desktop/素材 via IPC
            const api = window.nativesAPI;
            if (!api?.fs?.readFile || !api?.fs?.writeFileAtomic) return;
            const result = await api.fs.readFile(filePath);
            if (!result?.content) return;
            const fileName = filePath.split('/').pop() || filePath;
            await api.fs.writeFileAtomic(`~/Desktop/素材/${fileName}`, result.content);
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

// ── Module Details panel (STYLE-3) ──

function ModuleDetails({ moduleId, locale }: { moduleId: string; locale: Locale }) {
  const [mod, setMod] = useState<{ name: string; version: string; enabled: number; state: string; description?: string; author?: string } | null>(null);
  const [modulePerms, setModulePerms] = useState<Array<{ module_id: string; permission: string; granted: number }>>([]);
  useEffect(() => {
    async function load() {
      try {
        const api = window.nativesAPI;
        if (!api?.module?.list) return;
        const list = await api.module.list();
        if (Array.isArray(list)) {
          const found = (list as Array<{ id: string; name: string; version: string; enabled: number; state: string; description?: string; author?: string }>).find((m) => m.id === moduleId);
          if (found) setMod(found);
        }
        // P1-4: Load permissions
        if (api?.module?.listPermissions) {
          const perms = await api.module.listPermissions(moduleId);
          if (Array.isArray(perms)) setModulePerms(perms);
        }
      } catch { /* ignore */ }
    }
    load();
  }, [moduleId]);
  if (!mod) return <div style={{ padding: 16, color: 'var(--text-faint)', fontSize: 12 }}>{t(locale, 'common.loading')}</div>;
  return (
    <div style={{ padding: 16, fontSize: 12 }}>
      <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)', marginBottom: 16 }}>{mod.name}</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <InfoRow label={t(locale, 'workshop.templateId')} value={moduleId} />
        <InfoRow label={t(locale, 'store.installed')} value={t(locale, mod.enabled ? 'workshop.enabled' : 'workshop.disabled')} />
        {mod.version && <InfoRow label={t(locale, 'store.version')} value={'v' + mod.version} />}
        {mod.author && <InfoRow label={t(locale, 'store.author')} value={mod.author} />}
        {mod.description && <InfoRow label={t(locale, 'store.description')} value={mod.description} />}
      </div>
      {/* P1-4: Permissions list */}
      {modulePerms.length > 0 && (
        <div style={{ marginTop: 16 }}>
          <div style={{ fontSize: 10, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: 6 }}>
            Permissions
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            {modulePerms.map((perm) => (
              <div key={perm.permission} style={{
                display: 'flex', alignItems: 'center', gap: 8,
                padding: '6px 8px', background: 'var(--bg-3,#1c1e17)',
                borderRadius: 4, fontSize: 11,
              }}>
                <span style={{ color: perm.granted ? 'var(--accent,#cdf24b)' : 'var(--text-faint)' }}>
                  {perm.granted ? '✓' : '✗'}
                </span>
                <span style={{ color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>{perm.permission}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Keyboard shortcut help overlay (Cmd+/) */}
      <ShortcutHelp />
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div style={{ fontSize: 10, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: 2 }}>{label}</div>
      <div style={{ color: 'var(--text)', wordBreak: 'break-all' }}>{value}</div>
    </div>
  );
}
