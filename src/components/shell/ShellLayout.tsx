'use client';

import { startTransition, useState, useEffect, useRef, useCallback, lazy, Suspense, memo } from 'react';
import { t, type Locale } from '@/i18n';
import Sidebar from './Sidebar';
import RightPanel from './RightPanel';
import type { RightPanelMode } from './RightPanel';
import NotificationPanel from './NotificationPanel';
import Header from './Header';
import TerminalPanel from './Terminal';
import CommandPalette from './CommandPalette';
import ErrorBoundary from '@/components/ui/ErrorBoundary';
import ShortcutHelp from '@/components/ui/ShortcutHelp';
import SettingsPage from './SettingsPage';
import ControlHubWidget from './ControlHubWidget';
import { applyTheme } from '@/lib/theme-engine';

const MemoizedSidebar = memo(Sidebar);
const MemoizedHeader = memo(Header);
import { getIframeManager } from '@/lib/iframe-manager';
import ScreenshotCard from '@/components/screenshot/ScreenshotCard';
import AnnotationEditor from '@/components/screenshot/AnnotationEditor';
import ReleaseWizardDialog from '@/components/release/ReleaseWizardDialog';
import UpdateNotification from '@/components/update/UpdateNotification';
import UsernameOnboarding from '@/components/onboarding/UsernameOnboarding';
import { classifyError } from '@/lib/error-classifier';
import { useFollowMode } from '@/lib/follow-mode';
import { pushRecentModule } from '@/lib/recent-modules';
import LiquidGlass from '@/components/ui/LiquidGlass';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';
import { getHttpPort } from '@/lib/natives-http-port';
import ModuleDetails from './ModuleDetails';
import '@/types'; // ensure Window.nativesAPI type
import { Edit2, Eye, Radio } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { getExt, isMarkdownFile, isCsvFile, isArchiveFile } from '@/lib/follow-mode';
import { BUILTIN_TOOLS, getBuiltinTool } from '@/lib/builtin-tools';
import type { FileEntry } from '@/types/file';
import type { PreviewSubMode } from '@/components/files/FilePreview';

// Lazy-loaded heavy page components — 0 cost until actually visited
const LazyWorkshopPage = lazy(() => import('./WorkshopPage'));
const LazyFileBrowser = lazy(() => import('@/components/files/FileBrowser'));
const LazyFilePreview = lazy(() => import('@/components/files/FilePreview'));
const LazyAiWorkbench = lazy(() => import('@/components/ai/AiWorkbench'));
const LazyToolsPage = lazy(() => import('@/components/tools/ToolsPage'));
const LazyModulesPage = lazy(() => import('@/app/modules/page'));

// Lazy fallback
// 预创建内置工具的懒加载组件，避免在 render 中创建
const BUILTIN_LAZY_MAP: Record<string, React.LazyExoticComponent<any>> = {};
// 未来有 editor/browser 面板时在此预创建，例如：
// BUILTIN_LAZY_MAP['editor'] = lazy(() => import('@/components/shell/EditorPanel'));

const LazyFallback = () => (
  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', gap: 16 }}>
    <MathCurveLoader size={48} />
  </div>
);

interface ShellState {
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  rightPanelMode: RightPanelMode;
  rightPanelWidth: number;
  previewSubMode: PreviewSubMode;
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
    previewSubMode: 'preview',
    terminalCollapsed: true,
    terminalHeight: 280,
    terminalMaximized: false,
    cmdkOpen: false,
  });

  // Derive initial view from URL pathname — the App Router page files are
  // no-op shells that dispatch a navigate event, but we need to set the
  // correct view synchronously on first render (before child effects fire).
  const [activeView, setActiveView] = useState<string>(() => {
    if (typeof window === 'undefined') return 'dashboard';
    const path = window.location.pathname.replace(/\/+$/, '') || '/';
    const routingTable: Record<string, string> = {
      '/': 'dashboard',
      '/files': 'files',
      '/tools': 'tools',
      '/ai': 'ai',
      '/modules': 'modules',
    };
    return routingTable[path] || 'dashboard';
  });
  const [themeReady, setThemeReady] = useState(false);
  const [locale, setLocale] = useState<Locale>('zh');
  const [httpPort, setHttpPort] = useState<number | null>(null);
  const terminalSessionIdRef = useRef<string | null>(null);
  const iframeContainerRef = useRef<HTMLDivElement>(null);
  const activeModuleRef = useRef<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  // File preview state
  const [selectedFile, setSelectedFile] = useState<FileEntry | null>(null);
  const [editMode, setEditMode] = useState(false);

  // ── Global Background Visual Config (shared with ControlHub & Settings) ──
  const WALLPAPERS = [
    'https://images.unsplash.com/photo-1579033461380-adb47c3eb938?q=80&w=800&auto=format&fit=crop',
  ];

  const [visualConfig, setVisualConfig] = useState({
    blurAmount: 0.40,
    displacementScale: 64,
    saturation: 135,
    aberrationIntensity: 2,
    elasticity: 0,
    cornerRadius: 28,
    showWallpaper: true,
    showBlobs: true,
  });

  const CONFIG_DB_KEY = 'settings:controlHubVisuals';

  useEffect(() => {
    async function loadConfig() {
      try {
        const api = window.nativesAPI;
        if (!api?.db?.get) return;
        const savedVisuals = await api.db.get(CONFIG_DB_KEY);
        if (savedVisuals) {
          const saved = JSON.parse(savedVisuals as string);
          if (saved) {
            setVisualConfig((prev) => ({
              ...prev,
              ...(typeof saved.blurAmount === 'number' && { blurAmount: saved.blurAmount }),
              ...(typeof saved.displacementScale === 'number' && { displacementScale: saved.displacementScale }),
              ...(typeof saved.saturation === 'number' && { saturation: saved.saturation }),
              ...(typeof saved.aberrationIntensity === 'number' && { aberrationIntensity: saved.aberrationIntensity }),
              ...(typeof saved.elasticity === 'number' && { elasticity: saved.elasticity }),
              ...(typeof saved.cornerRadius === 'number' && { cornerRadius: saved.cornerRadius }),
              ...(typeof saved.showWallpaper === 'boolean' && { showWallpaper: saved.showWallpaper }),
              ...(typeof saved.showBlobs === 'boolean' && { showBlobs: saved.showBlobs }),
            }));
          }
        }
      } catch { /* no-op */ }
    }

    loadConfig();

    // Listen to local visual config changes (for instant drag updates in same window)
    const handleLocalConfigChange = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail) {
        setVisualConfig((prev) => ({
          ...prev,
          ...detail,
        }));
      }
    };
    window.addEventListener('visual-config-changed', handleLocalConfigChange);

    let unsubscribe: (() => void) | undefined;
    try {
      if (window.nativesAPI?.onDbStateChanged) {
        unsubscribe = window.nativesAPI.onDbStateChanged((_event, channel, data: any) => {
          if (channel === 'module_data' && data?.key === CONFIG_DB_KEY) {
            loadConfig();
          }
        });
      }
    } catch { /* no-op */ }

    return () => {
      window.removeEventListener('visual-config-changed', handleLocalConfigChange);
      if (unsubscribe) unsubscribe();
    };
  }, []);

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

  // ── stateRef: 追踪最新布局状态但不触发重新渲染 ──
  // 用于 beforeunload 持久化，避免拖拽时导致 init  Effect 反复重绑
  const stateRef = useRef(state);
  useEffect(() => { stateRef.current = state; }, [state]);

  // FOUC guard + locale/theme init + state persistence LOAD（只执行一次）
  useEffect(() => {
    // ── CRITICAL: Signal theme readiness IMMEDIATELY ──
    // The Tauri window starts with visible:false. The theme_ready_signal
    // command calls window.show(). If this is delayed or blocked by any
    // async operation (getHttpPort, db.get, etc.), the window stays
    // hidden → appears as white screen.
    //
    // Solution: Fire themeReady() FIRST, before any async work. Even if
    // subsequent init fails, the window is already visible.
    startTransition(() => { setThemeReady(true); });
    try {
      window.nativesAPI?.themeReady();
    } catch {
      // Browser dev mode — no Tauri window to show
    }

    async function initSettings() {
      const api = window.nativesAPI;
      if (!api) return;

      try {
        const savedTheme = await api.getTheme();
        if (savedTheme) applyTheme(savedTheme);
      } catch (err) {
        console.error('[Shell] Failed to load saved theme:', err);
        applyTheme('terminal-volt');
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

      // Load HTTP port for module iframe serving
      try {
        const port = await getHttpPort();
        setHttpPort(port);
      } catch { /* use default */ }

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
    }
    initSettings();
  }, []);

  // beforeunload 持久化（只挂一次，不依赖 state 变化重绑）
  // 通过 stateRef 读取最新布局值，避免拖拽时重新触发生成 Effect
  useEffect(() => {
    const handleBeforeUnload = () => {
      try {
        const api = window.nativesAPI;
        const s = stateRef.current; // 读取 stateRef，不产生依赖追踪
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
    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload);
    };
  }, []);

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
      startTransition(() => { setNeedsOnboarding(false); });
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
      const url = `http://localhost:${httpPort}/modules/${moduleId}/index.html`;
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
      // eslint-disable-next-line react-hooks/immutability
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
      else if (view === 'ai') setActiveView('ai');
      else if (view === 'tools') setActiveView('tools');
      else if (view === 'modules') setActiveView('modules');
      else if (typeof view === 'string' && view.startsWith('files')) {
        setActiveView('files');
        // Parse query params for path navigation (e.g. files?path=/dir&select=/file)
        const qIndex = view.indexOf('?');
        if (qIndex >= 0) {
          const params = new URLSearchParams(view.slice(qIndex + 1));
          const navPath = params.get('path');
          if (navPath) {
            // Store for FileBrowser to pick up after mount
            (window as any).__pendingNavigateFiles = navPath;
            window.dispatchEvent(new CustomEvent('navigate-files', { detail: navPath }));
          }
        }
      }
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

  const setPreviewSubMode = useCallback((mode: PreviewSubMode) => {
    setState((prev) => ({ ...prev, previewSubMode: mode }));
  }, []);
      // eslint-disable-next-line react-hooks/preserve-manual-memoization

    // eslint-disable-next-line react-hooks/preserve-manual-memoization
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
      const installResult = result as { success?: boolean; error?: string; moduleId: string };
      if (!installResult.success && installResult.error) {
        console.error('[Shell] Module install failed:', installResult.error);
      }
    } catch (err) {
      console.error('[Shell] Module install error:', err);
    }
  }, []);

  const handleModuleSelect = useCallback((moduleId: string) => {
    if (moduleId === '__dashboard__') {
      setActiveView('dashboard');
    } else if (moduleId === '__settings__') {
      setActiveView('settings');
    } else if (moduleId === '__workshop__') {
      setActiveView('workshop');
    } else if (moduleId === '__notifications__') {
      toggleRightPanel('notifications');
    } else if (moduleId.startsWith('__files__:')) {
      // Navigate file browser to a specific path
      const path = moduleId.slice(10);
      setActiveView('files');
      // Store path for FileBrowser to pick up after mount (race condition fix)
      (window as any).__pendingNavigateFiles = path;
      // Also dispatch event for already-mounted FileBrowser
      window.dispatchEvent(new CustomEvent('navigate-files', { detail: path }));
    } else if (moduleId.startsWith('builtin:')) {
      setActiveView(moduleId);
      const toolId = moduleId.slice('builtin:'.length);
      if (toolId === 'terminal') {
        (async () => {
          try {
            const list = await window.nativesAPI?.builtinTool?.list?.();
            const entry = list?.find((t: { id: string }) => t.id === 'terminal');
            if (entry?.driver === 'ghostty') {
              const running = await window.nativesAPI?.builtinTool?.ghosttyIsRunning?.();
              if (running) {
                await window.nativesAPI?.builtinTool?.ghosttyFocus?.();
              } else {
                await window.nativesAPI?.builtinTool?.launch?.('ghostty');
              }
            } else {
              setState((s) => ({ ...s, terminalCollapsed: false }));
            }
          } catch {
            setState((s) => ({ ...s, terminalCollapsed: false }));
          }
        })();
      }
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

  // Sync preview when file is renamed or trashed via FileBrowser context menu
  useEffect(() => {
    const handleRenamed = (e: Event) => {
      const { oldPath, newPath } = (e as CustomEvent).detail || {};
      if (!selectedFile || !oldPath || !newPath) return;
      if (selectedFile.path === oldPath) {
        const newName = newPath.split('/').pop() || selectedFile.name;
        setSelectedFile({ ...selectedFile, path: newPath, name: newName });
      }
    };
    const handleTrashed = (e: Event) => {
      const { path } = (e as CustomEvent).detail || {};
      if (selectedFile && selectedFile.path === path) {
        setSelectedFile(null);
        setRightPanelMode('closed');
      }
    };
    window.addEventListener('file-renamed', handleRenamed);
    window.addEventListener('file-trashed', handleTrashed);
    return () => {
      window.removeEventListener('file-renamed', handleRenamed);
      window.removeEventListener('file-trashed', handleTrashed);
    };
  }, [selectedFile, setRightPanelMode]);

  // Widget mode check — render only the ControlHub on transparent background
  const isWidgetMode = typeof window !== 'undefined' && window.location.search.includes('mode=widget');

  const renderMainContent = () => {
    // Builtin tool routing: builtin:terminal → open bottom panel, others → lazy content
    if (activeView.startsWith('builtin:')) {
      const toolId = activeView.slice('builtin:'.length);
      const toolDef = getBuiltinTool(toolId);

      // Terminal = bottom panel (special case)
      if (toolId === 'terminal') {
        // 面板展开已在 handleModuleSelect 中处理，此处仅返回内容
        return children; // show dashboard behind the panel
      }

      // Other builtin tools with displayMode 'content' — lazy load their component
      if (toolDef?.componentPath && toolDef.displayMode === 'content') {
        const LazyComponent = BUILTIN_LAZY_MAP[toolDef.id];
        if (LazyComponent) {
          return (
            <Suspense fallback={<LazyFallback />}>
              <LazyComponent />
            </Suspense>
          );
        }
        // fallback: 无预创建的懒组件，显示提示
        return <div style={{ padding: 40, color: 'var(--text-dim)' }}>Component not registered: {toolDef.componentPath}</div>;
      }

      // External-only tool (no embedded view) — launch externally
      if (toolDef) {
        // Read driver from DB and launch
        (async () => {
          try {
            const list = await window.nativesAPI?.builtinTool?.list?.();
            const entry = list?.find((t: { id: string }) => t.id === toolId);
            if (entry?.driver && entry.driver !== 'native') {
              await window.nativesAPI?.builtinTool?.launch?.(entry.driver);
            }
          } catch { /* ignore */ }
        })();
        return children;
      }

      return children;
    }

    switch (activeView) {
      case 'settings':
        return <SettingsPage />;
      case 'workshop':
        return <Suspense fallback={<LazyFallback />}><LazyWorkshopPage onInstall={handleInstallModule} /></Suspense>;
      case 'files':
        return <Suspense fallback={<LazyFallback />}><LazyFileBrowser onFileSelect={handleFileSelect} /></Suspense>;
      case 'ai':
        return <Suspense fallback={<LazyFallback />}><LazyAiWorkbench /></Suspense>;
      case 'tools':
        return <Suspense fallback={<LazyFallback />}><LazyToolsPage /></Suspense>;
      case 'modules':
        return <Suspense fallback={<LazyFallback />}><LazyModulesPage /></Suspense>;
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
  // Bypassed to directly render the demo
  // if (needsOnboarding === null) return null;
  // if (needsOnboarding) {
  //   return <UsernameOnboarding locale={locale} onComplete={() => setNeedsOnboarding(false)} />;
  // }

  // Widget mode — bypass chrome, render ControlHub directly
  if (isWidgetMode) {
    return (
      <div className="w-full h-full flex items-center justify-center bg-transparent">
        <ErrorBoundary>
          <ControlHubWidget />
        </ErrorBoundary>
      </div>
    );
  }

  return (
    <div className="vibe-canvas w-full h-full p-[1.125rem] flex gap-4 overflow-visible box-border relative isolate" style={{ opacity: themeReady ? 1 : 0 }}>
      {/* ── 全宽透明拖拽条 — absolute 定位 + 负 margin 穿透父 padding ── */}
      <div
        data-tauri-drag-region
        className="absolute top-0 z-50"
        style={{ left: '-1.125rem', right: '-1.125rem', height: '28px' }}
      />
      {/* ── Global Dynamic Background Layer ── */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden rounded-[1.125rem] z-0" style={{ zIndex: 0 }}>
        {visualConfig.showWallpaper && (
          <div
            className="absolute inset-0 bg-cover bg-center transition-all duration-[500ms] ease-in-out"
            style={{
              backgroundImage: `url(${WALLPAPERS[0]}), radial-gradient(at 10% 20%, rgba(0, 255, 156, 0.18) 0px, transparent 50%), radial-gradient(at 90% 10%, rgba(255, 121, 63, 0.22) 0px, transparent 50%), radial-gradient(at 50% 80%, rgba(0, 221, 255, 0.15) 0px, transparent 50%), linear-gradient(135deg, #0d0f12 0%, #1c2027 100%)`,
            }}
          />
        )}

        {visualConfig.showBlobs && (
          <div className="liquid-blob-wrapper absolute inset-0 z-10">
            <div className="liquid-blob liquid-blob-1" />
            <div className="liquid-blob liquid-blob-2" />
            <div className="liquid-blob liquid-blob-3" />
          </div>
        )}

        {/* Global WebGL Liquid Glass Canvas Layer */}
        <div className="absolute inset-0 opacity-40 z-20">
          <LiquidGlass
            isActive={true}
            className="w-full h-full border-none bg-transparent shadow-none animate-none"
            blurAmount={visualConfig.blurAmount}
            displacementScale={visualConfig.displacementScale}
            saturation={visualConfig.saturation}
            aberrationIntensity={visualConfig.aberrationIntensity}
            elasticity={visualConfig.elasticity}
          >
            <div className="w-full h-full" />
          </LiquidGlass>
        </div>
      </div>

      {/* Left: Sidebar */}
      <div
        className="h-full shrink-0 transition-[width] duration-200 relative z-10"
        style={{ width: state.sidebarCollapsed ? 0 : state.sidebarWidth, overflow: state.sidebarCollapsed ? 'hidden' : undefined }}
      >
        <MemoizedSidebar
          isCollapsed={state.sidebarCollapsed}
          onToggle={toggleSidebar}
          width={state.sidebarWidth}
          onResize={(w) => setState((prev) => ({ ...prev, sidebarWidth: w }))}
          activeModuleId={activeView}
          onModuleSelect={handleModuleSelect}
          onNotificationClick={() => toggleRightPanel('notifications')}
          locale={locale}
        />
      </div>

      {/* Right: Workspace */}
      <div className="flex-1 flex flex-col min-w-0 h-full box-border relative z-10">
        {/* ↓ relative z-20 确保 header 的下拉菜单不被 content panel 遮住 */}
        <div className="mb-3 relative z-20">
          <MemoizedHeader
            activeView={activeView}
            sidebarCollapsed={state.sidebarCollapsed}
            onToggleSidebar={toggleSidebar}
          />
        </div>

        {/* Main Content — conditional bottom margin to preserve gap when terminal is visible */}
        <div className={`flex-1 vibe-content-panel min-w-0 overflow-hidden relative${state.terminalCollapsed ? '' : ' mb-3'}`}>
          <div ref={contentRef} id="main-content" tabIndex={-1} style={{ width: '100%', height: '100%', outline: 'none' }}>
            <ErrorBoundary>
              {renderMainContent()}
            </ErrorBoundary>
          </div>
          {/* Portal target for content-area overlays — covers only the content panel */}
          <div id="content-overlay-root" style={{ position: 'absolute', inset: 0, zIndex: 50, pointerEvents: 'none' }} />
        </div>

        {/* Terminal — bottom of workspace column */}
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
      </div>

      {state.rightPanelMode !== 'closed' && (
        <RightPanel
          mode={state.rightPanelMode}
          onModeChange={setRightPanelMode}
          previewSubMode={state.previewSubMode}
          onPreviewSubModeChange={setPreviewSubMode}
          width={state.rightPanelWidth}
          onResize={(w) => setState((prev) => ({ ...prev, rightPanelWidth: w }))}
          title={state.rightPanelMode === 'file-preview' && selectedFile ? selectedFile.name : undefined}
          extraHeaderContent={
            state.rightPanelMode === 'file-preview' && selectedFile && state.previewSubMode === 'preview'
              ? (() => {
                  const ext = getExt(selectedFile.name);
                  const isCode = selectedFile.kind === 'text' && !isMarkdownFile(selectedFile.name) && !isCsvFile(selectedFile.name);
                  if (!isCode) return undefined;
                  return (
                    <button
                      className="flex items-center justify-center p-1.5 rounded-lg text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)] transition-all"
                      onClick={() => setEditMode(!editMode)}
                      title={editMode ? 'View mode' : 'Edit mode'}
                    >
                      {editMode ? <Eye size={13} /> : <Edit2 size={13} />}
                    </button>
                  );
                })()
              : undefined
          }
        >
          {state.rightPanelMode === 'notifications' && (
            <NotificationPanel locale={locale} />
          )}
          {state.rightPanelMode === 'file-preview' && selectedFile && (
            <Suspense fallback={<LazyFallback />}>
              <LazyFilePreview
                entry={selectedFile}
                subMode={state.previewSubMode}
                editMode={editMode}
                onEditModeChange={setEditMode}
                onClose={() => {
                  setSelectedFile(null);
                  setRightPanelMode('closed');
                  setEditMode(false);
                }}
              />
            </Suspense>
          )}
          {state.rightPanelMode === 'module-details' && activeView.startsWith('module:') && (
            <ModuleDetails moduleId={activeView.slice(7)} locale={locale} />
          )}
        </RightPanel>
      )}
      <CommandPalette
        isOpen={state.cmdkOpen}
        onClose={() => setState((prev) => ({ ...prev, cmdkOpen: false }))}
    // eslint-disable-next-line react-hooks/refs
        onSelect={handleModuleSelect}
        onToggleTerminal={toggleTerminal}
    // eslint-disable-next-line react-hooks/refs
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
            const readResult = result as { content?: string };
            if (!readResult?.content) return;
            const fileName = filePath.split('/').pop() || filePath;
            await api.fs.writeFileAtomic(`~/Desktop/素材/${fileName}`, readResult.content);
          } catch { /* ignore in browser mode */ }
        }}
        onAnnotate={async (filePath) => {
          // Load image as data URL for the annotation editor (CSP-safe, no file://)
          setAnnotatingFile(filePath);
          try {
            const api = window.nativesAPI;
            if (api?.thumbnail?.generate) {
              const dataUrl = await api.thumbnail.generate(filePath, 0) as unknown as string;
              if (dataUrl) {
                setAnnotationImageUrl(dataUrl);
                return;
              }
            }
            if (api?.fs?.readFile) {
              const result = await api.fs.readFile(filePath) as any;
              if (result?.content && result?.encoding === 'base64') {
                const ext = (filePath.split('.').pop() || 'png').toLowerCase();
                const mime = ext === 'jpg' || ext === 'jpeg' ? 'image/jpeg' : 'image/png';
                setAnnotationImageUrl(`data:${mime};base64,${result.content}`);
                return;
              }
            }
          } catch (err) {
            console.error('[Shell] Failed to load image for annotation:', err);
          }
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
                const annotateResult = result as { success?: boolean; path?: string };
                if (annotateResult.success) {
                  console.log('[Shell] Annotated image saved:', annotateResult.path);
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
