'use client';

import { startTransition, useState, useEffect, useCallback, memo, lazy, Suspense } from 'react';
import { type Locale } from '@/i18n';
import Sidebar, { SIDEBAR_COLLAPSED_WIDTH, clampSidebarWidth } from './Sidebar';
import RightPanel, { clampRightPanelWidth } from './RightPanel';
import type { RightPanelMode } from './RightPanel';
import NotificationPanel from './NotificationPanel';
import Header from './Header';
import TerminalPanel from './Terminal';
import ErrorBoundary from '@/components/ui/ErrorBoundary';
import ControlHubWidget from './ControlHubWidget';
import MainContent from './MainContent';
import { applyTheme } from '@/lib/theme-engine';

const MemoizedSidebar = memo(Sidebar);
const MemoizedHeader = memo(Header);
// V1.0: LiquidGlass 已退役（纯色 Surface 体系）
import { getHttpPort } from '@/lib/natives-http-port';
import { useShellState } from './useShellState';
import '@/types'; // ensure Window.nativesAPI type
import { Edit2, Eye } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { isCsvFile } from '@/lib/follow-mode';
import { navigateToFiles } from '@/lib/file-events';
import { onFollowChange } from '@/lib/follow-mode';
import { fsApi, hasNativeFiles, thumbnailApi } from '@/lib/files-api';
import type { FileEntry } from '@/types/file';
import type { PreviewSubMode } from '@/components/files/FilePreview';

// Right panel lazy imports (not in MainContent)
const LazyFilePreview = lazy(() => import('@/components/files/FilePreview'));
const LazyFollowRenderer = lazy(() => import('@/components/ai/FollowRenderer'));
const LazyCommandPalette = lazy(() => import('./CommandPalette'));
const LazyScreenshotCard = lazy(() => import('@/components/screenshot/ScreenshotCard'));
const LazyAnnotationEditor = lazy(() => import('@/components/screenshot/AnnotationEditor'));
const LazyReleaseWizardDialog = lazy(() => import('@/components/release/ReleaseWizardDialog'));
const LazyUpdateNotification = lazy(() => import('@/components/update/UpdateNotification'));
const LazyModuleDetails = lazy(() => import('./ModuleDetails'));
const LazyUsernameOnboarding = lazy(() => import('@/components/onboarding/UsernameOnboarding'));
const LazyFallback = () => (
  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', gap: 16 }}>
    <MathCurveLoader size={48} />
  </div>
);

function IdleUpdateNotification({ locale }: { locale: Locale }) {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const schedule = (window as Window & { requestIdleCallback?: (cb: () => void) => number }).requestIdleCallback;
    if (schedule) {
      const id = schedule(() => setReady(true));
      return () => (window as Window & { cancelIdleCallback?: (id: number) => void }).cancelIdleCallback?.(id);
    }
    const id = window.setTimeout(() => setReady(true), 1000);
    return () => window.clearTimeout(id);
  }, []);
  return ready ? <Suspense fallback={null}><LazyUpdateNotification locale={locale} /></Suspense> : null;
}
import { useLayoutEvents } from './hooks/useLayoutEvents';
import { useModuleEvents } from './hooks/useModuleEvents';
import { useFileEvents } from './hooks/useFileEvents';
import { isSettingsView, normalizeSettingsTarget } from './settings-navigation';

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

  const handleSidebarResize = useCallback((width: number) => {
    setState((prev) => ({ ...prev, sidebarWidth: clampSidebarWidth(width) }));
  }, [setState]);
  const handleRightPanelResize = useCallback((width: number) => {
    setState((prev) => ({ ...prev, rightPanelWidth: clampRightPanelWidth(width) }));
  }, [setState]);
  const handleTerminalResize = useCallback((height: number) => {
    setState((prev) => ({ ...prev, terminalHeight: height }));
  }, [setState]);

  // ── Event hooks ──
  useLayoutEvents({
    stateRef,
    layoutPersist: {
      sidebarWidth: state.sidebarWidth,
      sidebarCollapsed: state.sidebarCollapsed,
      terminalHeight: state.terminalHeight,
      terminalCollapsed: state.terminalCollapsed,
      rightPanelWidth: state.rightPanelWidth,
    },
    toggleTerminal,
    setState,
    setLocale,
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

  // ── 文件跟随（file-follow）：状态机产出 → 右面板 FollowRenderer ──
  // follow-mode.ts 的引擎（fs_watch 喂 followChange）在切到该档位时启动；
  // 这里只消费 onFollowChange：agent 刚写的文件自动出现在右面板实时渲染。
  const [followPath, setFollowPath] = useState<string | null>(null);
  useEffect(() => {
    return onFollowChange((path) => {
      setFollowPath(path);
      if (path) setRightPanelMode('follow');
    });
  }, [setRightPanelMode]);
  // 档位切离 file-follow 时收起跟随面板
  useEffect(() => {
    if (followMode !== 'file-follow' && stateRef.current.rightPanelMode === 'follow') {
      setRightPanelMode('closed');
      setFollowPath(null);
    }

  }, [followMode]);

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
        // 与主题引擎既定一致（dark）；原来落到 light 与引擎兜底反向
        applyTheme('dark');
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
                ...(typeof saved.sidebarWidth === 'number' && {
                  sidebarWidth: clampSidebarWidth(saved.sidebarWidth),
                }),
                ...(typeof saved.sidebarCollapsed === 'boolean' && { sidebarCollapsed: saved.sidebarCollapsed }),
                ...(typeof saved.terminalHeight === 'number' && { terminalHeight: saved.terminalHeight }),
                ...(typeof saved.terminalCollapsed === 'boolean' && { terminalCollapsed: saved.terminalCollapsed }),
                ...(typeof saved.rightPanelWidth === 'number' && {
                  rightPanelWidth: clampRightPanelWidth(saved.rightPanelWidth),
                }),
              }));
            }
          }
        }
      } catch (err) {
        console.warn('[Shell] Failed to load sidebar state:', err);
      }
      // V1.0: 背景 wallpaper / blob / WebGL 视觉配置加载已废弃。
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
    // Check settings navigation first — catches __settings__, settings, settings:*
    const settingsTarget = normalizeSettingsTarget(moduleId);
    if (settingsTarget) {
      setActiveView(settingsTarget);
      // Close module-details when entering settings; preserve notifications and file-preview
      setState((prev) =>
        prev.rightPanelMode === 'module-details'
          ? { ...prev, rightPanelMode: 'closed' }
          : prev,
      );
      return;
    }

    if (moduleId === '__dashboard__') {
      setActiveView('dashboard');
    } else if (moduleId === '__workshop__' || moduleId === 'modules' || moduleId === 'store') {
      // Personal Creations — canonical view key remains `modules` for main menu / deep links
      setActiveView('modules');
    } else if (moduleId === '__assistant__') {
      setActiveView('assistant');
    } else if (moduleId === '__jobs__' || moduleId === 'jobs') {
      setActiveView('jobs');
    } else if (moduleId === '__capabilities__' || moduleId === 'capabilities') {
      setActiveView('capabilities');
    } else if (moduleId === '__library__' || moduleId === 'library') {
      setActiveView('library');
    } else if (moduleId === 'files' || moduleId === 'ai' || moduleId === 'tools' || moduleId === 'assistant' || moduleId === 'dashboard') {
      // 命令面板等处传裸视图 id；此前会掉进兜底分支被当成 `module:<id>` iframe 打开
      setActiveView(moduleId);
    } else if (moduleId === '__notifications__') {
      toggleRightPanel('notifications');
    } else if (moduleId.startsWith('__files__:')) {
      // Navigate file browser to a specific path
      const path = moduleId.slice(10);
      setActiveView('files');
      // 统一入口：dispatch + 挂载竞态 pending 兜底（替代 window.__pendingNavigateFiles）
      navigateToFiles(path);
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
  }, [toggleRightPanel, setRightPanelMode, setActiveView, setState]);

  // File selection handler — opens preview in right panel
  const handleFileSelect = useCallback((entry: FileEntry) => {
    setSelectedFile(entry);
    setRightPanelMode('file-preview');
  }, [setRightPanelMode]);

  // Widget mode check — render only the ControlHub on transparent background
  const isWidgetMode = typeof window !== 'undefined' && window.location.search.includes('mode=widget');
  const isSettingsMode = isSettingsView(activeView);
  const effectiveSidebarCollapsed = isSettingsMode ? false : state.sidebarCollapsed;


  // P1-5: 首次运行未设置用户名时进入引导页（此前被 demo 时期的注释旁路，导致
  // UsernameOnboarding 成为死代码、主页问候语无名可用）
  if (!isWidgetMode && needsOnboarding) {
    return (
      <Suspense fallback={null}>
        <LazyUsernameOnboarding locale={locale} onComplete={() => setNeedsOnboarding(false)} />
      </Suspense>
    );
  }

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
    <>
      {/* ── V1.0 纯色背景，无 grain / blob / WebGL Liquid Glass ── */}

      <div className="w-full h-full" style={{ opacity: themeReady ? 1 : 0 }}>
      <div className="w-full h-full bg-[var(--background)] p-3 flex gap-3 overflow-visible box-border relative isolate">
      {/* ── 工作区顶部拖拽条：必须从侧栏右侧开始，否则会盖住「折叠」按钮 ──
          侧栏 z-50 > 本层 z-30，双重保证侧栏标题栏可点。 */}
      <div
        data-tauri-drag-region
        className="absolute top-0 z-30"
        style={{
          // p-3(12) + sidebar + gap-3(12)
          left: `${12 + (effectiveSidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : state.sidebarWidth) + 12}px`,
          right: '0px',
          height: '20px',
        }}
      />
      {/* ── V1.0 已移除：wallpaper / liquid-blob / WebGL LiquidGlass 全局背景层 ── */}

      {/* Left: Sidebar — collapsed keeps an icon rail, not width 0 */}
      <div
        className="h-full shrink-0 transition-[width] duration-200 relative z-50"
        style={{
          width: effectiveSidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : state.sidebarWidth,
          // overflow visible so the right-edge drag handle is hit-testable
          overflow: 'visible',
        }}
      >
        <MemoizedSidebar
          isCollapsed={effectiveSidebarCollapsed}
          onToggle={toggleSidebar}
          width={state.sidebarWidth}
          onResize={handleSidebarResize}
          activeModuleId={activeView}
          onModuleSelect={handleModuleSelect}
          onNotificationClick={() => toggleRightPanel('notifications')}
          locale={locale}
        />
      </div>

      {/* Right: Workspace */}
      <div className="flex-1 flex flex-col min-w-0 h-full box-border relative z-10">
        {/* Main Content — conditional bottom margin to preserve gap when terminal is visible */}
        <div
          className={`flex-1 surface-section min-w-0 overflow-hidden relative flex flex-col${state.terminalCollapsed || isSettingsMode ? '' : ' mb-3'}`}
          style={{ paddingTop: 0 }}
        >
          {/* ↓ relative z-20 确保 header 的下拉菜单不被 content panel 遮住 */}
          {activeView !== 'dashboard' && !isSettingsMode && (
            <div className="relative z-20 shrink-0">
              <MemoizedHeader
                activeView={activeView}
                sidebarCollapsed={state.sidebarCollapsed}
                onToggleSidebar={toggleSidebar}
              />
            </div>
          )}

          <div ref={contentRef} id="main-content" tabIndex={-1} style={{ width: '100%', height: '100%', outline: 'none' }} className="flex-1 min-h-0">
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

        {/* Terminal — bottom of workspace column, hidden via CSS to preserve state */}
        <div className={isSettingsMode ? 'hidden' : 'contents'}>
          <TerminalPanel
            isCollapsed={state.terminalCollapsed}
            onToggle={toggleTerminal}
            height={state.terminalHeight}
            onResize={handleTerminalResize}
            isMaximized={state.terminalMaximized}
            onMaximizeToggle={toggleMaximized}
            onSessionCreated={(id) => { terminalSessionIdRef.current = id; }}
            followMode={followMode !== 'off'}
            onFollowModeToggle={cycleFollowMode}
          />
        </div>
      </div>

      {state.rightPanelMode !== 'closed' && (
        <div className={isSettingsMode ? 'hidden' : 'contents'}>
          <RightPanel
          mode={state.rightPanelMode}
          onModeChange={setRightPanelMode}
          previewSubMode={state.previewSubMode}
          onPreviewSubModeChange={setPreviewSubMode}
          width={state.rightPanelWidth}
          onResize={handleRightPanelResize}
          title={
            state.rightPanelMode === 'file-preview' && selectedFile
              ? selectedFile.name
              : state.rightPanelMode === 'follow' && followPath
                ? followPath.split('/').pop()
                : undefined
          }
          extraHeaderContent={
            state.rightPanelMode === 'file-preview' && selectedFile && state.previewSubMode === 'preview'
              ? (() => {
                  const editableText = selectedFile.kind === 'text' && !isCsvFile(selectedFile.name);
                  if (!editableText) return undefined;
                  return (
                    <button
                      className="flex items-center justify-center p-1.5 rounded-lg text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-all"
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
                key={selectedFile.path}
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
            <Suspense fallback={<LazyFallback />}><LazyModuleDetails moduleId={activeView.slice(7)} locale={locale} /></Suspense>
          )}
          {state.rightPanelMode === 'follow' && (
            <Suspense fallback={<LazyFallback />}>
              <LazyFollowRenderer filePath={followPath} />
            </Suspense>
          )}
        </RightPanel>
        </div>
      )}
      <Suspense fallback={null}><LazyCommandPalette
        isOpen={state.cmdkOpen}
        onClose={() => setState((prev) => ({ ...prev, cmdkOpen: false }))}

        onSelect={handleModuleSelect}
        onToggleTerminal={toggleTerminal}

        terminalSessionId={terminalSessionIdRef.current}
      /></Suspense>

      {/* Phase 3: Screenshot Card */}
      <Suspense fallback={null}><LazyScreenshotCard
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
            // files-api 契约：fs 不可用（浏览器 dev）时静默跳过
            if (!hasNativeFiles()) return;
            const fileName = filePath.split('/').pop() || filePath;
            // 字节级复制（copy_entry 自动建父目录 + 同名去重）；
            // 禁止 readFile+writeFileAtomic 文本中转——会损坏 PNG 二进制
            await fsApi().copyEntry(filePath, `~/Desktop/素材/${fileName}`);
          } catch { /* ignore in browser mode */ }
        }}
        onAnnotate={async (filePath) => {
          // Load image as data URL for the annotation editor (CSP-safe, no file://)
          setAnnotatingFile(filePath);
          try {
            // files-api 契约：先探测能力再调用，保持原「缺哪个就跳过哪个」的降级链
            const thumbnail = (() => {
              try {
                return thumbnailApi();
              } catch {
                return null;
              }
            })();
            if (thumbnail) {
              const dataUrl = await thumbnail.generate(filePath, 0) as unknown as string;
              if (dataUrl) {
                setAnnotationImageUrl(dataUrl);
                return;
              }
            }
            if (hasNativeFiles()) {
              const result = await fsApi().readFile(filePath) as any;
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
      /></Suspense>

      {/* Phase 3: Annotation Editor */}
      {annotationImageUrl && annotatingFile && (
        <Suspense fallback={null}><LazyAnnotationEditor
          locale={locale}
          imageUrl={annotationImageUrl}
          onSave={async (dataUrl) => {
            try {
              const api = window.nativesAPI;
              if (api?.screenshot?.saveAnnotated) {
                await api.screenshot.saveAnnotated(dataUrl, annotatingFile.replace(/\.(png|jpg|jpeg|webp)$/, '-annotated.png'));
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
        /></Suspense>
      )}

      {/* Phase 3: Release Wizard */}
      <Suspense fallback={null}><LazyReleaseWizardDialog
        locale={locale}
        isOpen={releaseWizardOpen}
        onClose={() => setReleaseWizardOpen(false)}
      /></Suspense>

      {/* Phase 3: Update Notification */}
      <IdleUpdateNotification locale={locale} />
    </div>
    </div>
    </>
  );
}
