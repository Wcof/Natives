'use client';

import { startTransition, useState, useEffect, useCallback, memo, lazy, Suspense } from 'react';
import { type Locale } from '@/i18n';
import Sidebar from './Sidebar';
import RightPanel from './RightPanel';
import type { RightPanelMode } from './RightPanel';
import NotificationPanel from './NotificationPanel';
import Header from './Header';
import TerminalPanel from './Terminal';
import CommandPalette from './CommandPalette';
import ErrorBoundary from '@/components/ui/ErrorBoundary';
import ControlHubWidget from './ControlHubWidget';
import MainContent from './MainContent';
import { applyTheme } from '@/lib/theme-engine';

const MemoizedSidebar = memo(Sidebar);
const MemoizedHeader = memo(Header);
import ScreenshotCard from '@/components/screenshot/ScreenshotCard';
import AnnotationEditor from '@/components/screenshot/AnnotationEditor';
import ReleaseWizardDialog from '@/components/release/ReleaseWizardDialog';
import UpdateNotification from '@/components/update/UpdateNotification';
import LiquidGlass from '@/components/ui/LiquidGlass';
import { getHttpPort } from '@/lib/natives-http-port';
import ModuleDetails from './ModuleDetails';
import { useShellState } from './useShellState';
import '@/types'; // ensure Window.nativesAPI type
import { Edit2, Eye } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { getExt, isMarkdownFile, isCsvFile, isArchiveFile } from '@/lib/follow-mode';
import type { FileEntry } from '@/types/file';
import type { PreviewSubMode } from '@/components/files/FilePreview';

// Right panel lazy imports (not in MainContent)
const LazyFilePreview = lazy(() => import('@/components/files/FilePreview'));
const LazyFallback = () => (
  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', gap: 16 }}>
    <MathCurveLoader size={48} />
  </div>
);
import { useLayoutEvents } from './hooks/useLayoutEvents';
import { useModuleEvents } from './hooks/useModuleEvents';
import { useFileEvents } from './hooks/useFileEvents';

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
  const {
    state, setState, stateRef,
    activeView, setActiveView,
    themeReady, setThemeReady,
    locale, setLocale,
    httpPort, setHttpPort,
    terminalSessionIdRef,
    iframeContainerRef,
    activeModuleRef,
    contentRef,
    selectedFile, setSelectedFile,
    editMode, setEditMode,
    visualConfig, setVisualConfig,
    followMode, cycleFollowMode,
    crashedModules, setCrashedModules,
    needsOnboarding, setNeedsOnboarding,
    annotatingFile, setAnnotatingFile,
    annotationImageUrl, setAnnotationImageUrl,
    releaseWizardOpen, setReleaseWizardOpen,
    toggleSidebar, toggleTerminal, toggleMaximized,
    toggleRightPanel,
    setRightPanelMode,
  } = useShellState();

  // ── Global Background Visual Config (shared with ControlHub & Settings) ──
  const WALLPAPERS = [
    'https://images.unsplash.com/photo-1579033461380-adb47c3eb938?q=80&w=800&auto=format&fit=crop',
  ];

  const CONFIG_DB_KEY = 'settings:controlHubVisuals';

  // ── Event hooks ──
  useLayoutEvents({
    setVisualConfig,
    stateRef,
    toggleTerminal,
    setState,
    setLocale,
    CONFIG_DB_KEY,
  });
  useModuleEvents({
    activeView,
    httpPort,
    iframeContainerRef,
    activeModuleRef,
    setCrashedModules,
    setReleaseWizardOpen,
    setActiveView,
  });
  useFileEvents({
    followMode,
    terminalSessionIdRef,
    selectedFile,
    setSelectedFile,
    setRightPanelMode,
  });

  // ── Global Background Visual Config (shared with ControlHub & Settings) ──

  // Crash state: track crashed modules for overlay display
  const [iframeReloadKey, setIframeReloadKey] = useState(0);

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

      // Load visual config (cornerRadius, blur, etc.)
      try {
        const api = window.nativesAPI;
        if (api?.db?.get) {
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
        }
      } catch { /* no-op */ }
    }
    initSettings();
  }, []);

  // Focus management: move focus to content area when view changes
  useEffect(() => {
    if (contentRef.current) {
      contentRef.current.focus();
    }
  }, [activeView]);

  const setPreviewSubMode = useCallback((mode: PreviewSubMode) => {
    setState((prev) => ({ ...prev, previewSubMode: mode }));
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

  // Widget mode check — render only the ControlHub on transparent background
  const isWidgetMode = typeof window !== 'undefined' && window.location.search.includes('mode=widget');


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
              <MainContent
                activeView={activeView}
                locale={locale}
                httpPort={httpPort}
                selectedFile={selectedFile}
                setSelectedFile={setSelectedFile}
                editMode={editMode}
                setEditMode={setEditMode}
                iframeReloadKey={0}
                terminalSessionId={terminalSessionIdRef.current}
                onFileSelect={handleFileSelect}
                iframeContainerRef={iframeContainerRef}
              >
                {children}
              </MainContent>
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
