'use client';

import { startTransition, useState, useEffect, useCallback, useRef, memo, lazy, Suspense } from 'react';
import { t, type Locale } from '@/i18n';
import Sidebar, { SIDEBAR_COLLAPSED_WIDTH, clampSidebarWidth } from './Sidebar';
import RightPanel, { clampRightPanelWidth } from './RightPanel';
import NotificationPanel from './NotificationPanel';
import Header from './Header';
import TerminalPanel from './Terminal';
import ErrorBoundary from '@/components/ui/ErrorBoundary';
import MainContent from './MainContent';

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
import type { PreviewSubMode } from '@/lib/preview/contracts';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { appsApi, type AppView, type DockResult, type SystemRunningState, type WebBounds } from '@/lib/tauri/apps';
import { reduceAppTarget, isWebTargetPresented, type ActiveAppTarget } from '@/lib/app-target';
import AppPresentationHost, { type AppPresentationBounds, type AppSystemCommand, type AppWebCommand } from './AppPresentationHost';

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

export default function ShellLayout({ children }: { children: React.ReactNode }) {
  const { toast } = useToast();
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
    setCrashedModules,
    needsOnboarding, setNeedsOnboarding,
    annotatingFile, setAnnotatingFile,
    annotationImageUrl, setAnnotationImageUrl,
    releaseWizardOpen, setReleaseWizardOpen,
    toggleSidebar, toggleTerminal, toggleMaximized,
    toggleRightPanel,
    setRightPanelMode,
  } = useShellState();

  // ── APPV2-T02：当前应用呈现目标（应用中心 V2；Shell 局部状态，不建全局 store） ──
  const [activeAppTarget, setActiveAppTarget] = useState<ActiveAppTarget | null>(null);
  const activeAppTargetRef = useRef<ActiveAppTarget | null>(null);
  const [systemRunningState, setSystemRunningState] = useState<SystemRunningState | null>(null);
  const [systemDockResult, setSystemDockResult] = useState<DockResult | null>(null);
  const [systemNeedsRedock, setSystemNeedsRedock] = useState(false);
  const dockAttemptedRef = useRef(new Set<string>());
  const dockResultRef = useRef<DockResult | null>(null);
  const appSwitchSeqRef = useRef(0);
  useEffect(() => {
    activeAppTargetRef.current = activeAppTarget;
  }, [activeAppTarget]);
  useEffect(() => {
    dockResultRef.current = systemDockResult;
  }, [systemDockResult]);

  // APPV2-T03：内容区矩形缓存（Host ResizeObserver 测量）；open 时携带，resize 时下发
  const webBoundsRef = useRef<WebBounds | null>(null);
  const handleAppBoundsChange = useCallback((bounds: AppPresentationBounds | null) => {
    webBoundsRef.current = bounds;
    if (!bounds) return;
    const target = activeAppTargetRef.current;
    // 仅 web 目标下发动态 bounds；label 不存在时 Host 端静默 no-op，open 会携带 bounds。
    if (target && target.kind === 'web_application') {
      void appsApi
        .webSetBounds(target.appId, {
          x: bounds.x,
          y: bounds.y,
          width: bounds.width,
          height: bounds.height,
        })
        .catch(() => {});
    } else if (target?.kind === 'system_application' && target.phase === 'presented') {
      if (!dockAttemptedRef.current.has(target.appId)) {
        dockAttemptedRef.current.add(target.appId);
        void appsApi.systemDock(target.appId, bounds).then((result) => {
          if (activeAppTargetRef.current?.appId === target.appId) {
            setSystemDockResult(result);
            setSystemNeedsRedock(false);
          }
        }).catch((error) => {
          if (activeAppTargetRef.current?.appId === target.appId) {
            setSystemDockResult({ status: 'failed', capability: 'available', message: String(error) });
          }
        });
      } else if (dockResultRef.current?.status === 'docked') {
        setSystemNeedsRedock(true);
      }
    }
  }, []);

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

  // Locale init + state persistence LOAD（只执行一次；主题由 ThemeProvider / AppearanceCoordinator 统管）
  useEffect(() => {
    startTransition(() => { setThemeReady(true); });

    async function initSettings() {
      const api = window.nativesAPI;
      if (!api) return;

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

  // APPV2-T02：点击侧边栏应用 = 解析 AppView → switching → open → presented / 可恢复 error。
  // 选中态（activeView = apps:item:<id>）只在真实切换成功后建立；失败保留错误态可重试。
  const openAppTarget = useCallback(
    async (appId: string) => {
      const requestSeq = ++appSwitchSeqRef.current;
      // APPV2-T04：Web→Web 切换时先 hide 旧目标（永不 terminate）；同 appId 重试不 hide。
      const prevTarget = activeAppTargetRef.current;
      if (prevTarget && isWebTargetPresented(prevTarget) && prevTarget.appId !== appId) {
        void appsApi.webHide(prevTarget.appId).catch(() => {});
      }
      if (prevTarget?.kind === 'system_application' && prevTarget.appId !== appId) {
        void appsApi.systemHide(prevTarget.appId).catch(() => {});
      }
      setSystemRunningState(null);
      setSystemDockResult(null);
      setSystemNeedsRedock(false);
      setActiveAppTarget((prev) => reduceAppTarget(prev, { type: 'request', appId }));
      let view: AppView | null = null;
      try {
        view = await appsApi.getView(appId);
      } catch {
        view = null;
      }
      if (requestSeq !== appSwitchSeqRef.current) return;
      if (!view) {
        setActiveAppTarget((prev) =>
          reduceAppTarget(prev, { type: 'failed', appId, error: t(locale, 'appsPage.presentNotFound') }),
        );
        return;
      }
      setActiveAppTarget((prev) => reduceAppTarget(prev, { type: 'viewResolved', appId, view }));
      if (view.kind === 'local_project') return; // reducer 标记 unsupported（UI 层切割，数据保留）
      try {
        if (view.kind === 'web_application' && prevTarget?.kind === 'system_application') {
          await appsApi.activateHost();
        }
        // APPV2-T03：Web 目标「先 bounds 后 show」——携带 Host 实测内容区矩形。
        await appsApi.open(appId, webBoundsRef.current ?? undefined);
        if (requestSeq !== appSwitchSeqRef.current) {
          if (view.kind === 'web_application') await appsApi.webHide(appId).catch(() => false);
          else await appsApi.systemHide(appId).catch(() => false);
          return;
        }
        if (view.kind === 'system_application') {
          setSystemRunningState(await appsApi.systemObserve(appId));
        }
        setActiveAppTarget((prev) => reduceAppTarget(prev, { type: 'presented', appId }));
        setActiveView(`apps:item:${appId}`);
      } catch (err) {
        setActiveAppTarget((prev) =>
          reduceAppTarget(prev, { type: 'failed', appId, error: classifyError(err, { locale }).userMessage }),
        );
      }
    },
    [locale, setActiveView],
  );

  // 切离呈现：只 hide 受管 Web Surface（永不 terminate）；清空目标。
  const clearAppTarget = useCallback(() => {
    appSwitchSeqRef.current += 1;
    const prev = activeAppTargetRef.current;
    if (prev && isWebTargetPresented(prev)) {
      void appsApi.webHide(prev.appId).catch(() => {});
    } else if (prev?.kind === 'system_application' && prev.phase === 'presented') {
      void appsApi.systemHide(prev.appId).catch(() => {});
    }
    setActiveAppTarget(null);
  }, []);

  const handleAppSystemCommand = useCallback(
    async (appId: string, command: AppSystemCommand) => {
      try {
        if (command === 'settings') {
          await appsApi.systemOpenAccessibilitySettings();
          return;
        }
        if (command === 'redock') {
          const bounds = webBoundsRef.current;
          if (!bounds) return;
          const result = await appsApi.systemDock(appId, bounds);
          dockAttemptedRef.current.add(appId);
          setSystemDockResult(result);
          setSystemNeedsRedock(false);
        } else if (command === 'hide') {
          await appsApi.systemHide(appId);
        } else if (command === 'terminate') {
          await appsApi.stop(appId, 1);
        } else if (command === 'activate') {
          await appsApi.open(appId, webBoundsRef.current ?? undefined);
        }
        setSystemRunningState(await appsApi.systemObserve(appId));
      } catch (error) {
        toast(classifyError(error, { locale }).userMessage, 'error');
      }
    },
    [locale, toast],
  );

  const handleAppWebCommand = useCallback(
    (appId: string, command: AppWebCommand) => {
      if (command === 'close') {
        clearAppTarget();
        return;
      }
      const promise =
        command === 'reload'
          ? appsApi.webReload(appId)
          : command === 'back'
            ? appsApi.webBack(appId)
            : appsApi.webForward(appId);
      void promise.catch(() => {});
    },
    [clearAppTarget],
  );

  const handleModuleSelect = useCallback((moduleId: string) => {
    // APPV2-T02：任何非应用呈现的选中先清除当前目标（web surface 只 hide 不终止）
    if (!moduleId.startsWith('apps:item:')) clearAppTarget();
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
    } else if (moduleId === 'apps' || moduleId === '__apps__' || moduleId === '__workshop__' || moduleId === 'modules' || moduleId === 'store') {
      // Canonical entry is apps; legacy workshop/modules/store redirect to apps
      setActiveView('apps');
    } else if (moduleId === '__assistant__') {
      setActiveView('assistant');
    } else if (moduleId === '__jobs__' || moduleId === 'jobs') {
      setActiveView('jobs');
    } else if (moduleId === '__capabilities__' || moduleId === 'capabilities') {
      setActiveView('capabilities');
    } else if (moduleId === '__library__' || moduleId === 'library') {
      setActiveView('library');
    } else if (moduleId === 'files' || moduleId === 'ai' || moduleId === 'tools' || moduleId === 'assistant' || moduleId === 'dashboard' || moduleId === 'usage') {
      // 命令面板等处传裸视图 id；此前会掉进兜底分支被当成 `module:<id>` iframe 打开
      setActiveView(moduleId);
    } else if (moduleId === '__notifications__') {
      toggleRightPanel('notifications');
    } else if (moduleId.startsWith('apps:item:')) {
      const appId = moduleId.slice('apps:item:'.length);
      void openAppTarget(appId);
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
  }, [toggleRightPanel, setRightPanelMode, setActiveView, setState, clearAppTarget, openAppTarget]);

  // File selection handler — opens preview in right panel
  const handleFileSelect = useCallback((entry: FileEntry) => {
    setSelectedFile(entry);
    setRightPanelMode('file-preview');
  }, [setRightPanelMode]);

  // Widget mode check — render only the ControlHub on transparent background
  const isSettingsMode = isSettingsView(activeView);
  const effectiveSidebarCollapsed = isSettingsMode ? false : state.sidebarCollapsed;


  // P1-5: 首次运行未设置用户名时进入引导页（此前被 demo 时期的注释旁路，导致
  // UsernameOnboarding 成为死代码、主页问候语无名可用）
  if (needsOnboarding) {
    return (
      <Suspense fallback={null}>
        <LazyUsernameOnboarding locale={locale} onComplete={() => setNeedsOnboarding(false)} />
      </Suspense>
    );
  }

  return (
    <>
      {/* ── V1.0 纯色背景，无 grain / blob / WebGL Liquid Glass ── */}

      <div className="w-full h-full" style={{ opacity: themeReady ? 1 : 0 }}>
      <div className="shell-workspace w-full h-full flex overflow-visible box-border relative isolate">
      {/* ── 工作区顶部拖拽条：必须从侧栏右侧开始，否则会盖住「折叠」按钮 ──
          侧栏 z-50 > 本层 z-30，双重保证侧栏标题栏可点。 */}
      <div
        data-tauri-drag-region
        className="absolute top-0 z-30"
        style={{
          left: `${effectiveSidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : state.sidebarWidth}px`,
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
          className="flex-1 surface-section min-w-0 overflow-hidden relative flex flex-col"
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
            {activeAppTarget ? (
              // APPV2-T02：应用呈现态（Web 工具条/状态层；macOS 承载状态；child WebView 覆盖内容矩形）
              <ErrorBoundary>
                <AppPresentationHost
                  target={activeAppTarget}
                  locale={locale}
                  onRetry={(appId) => void openAppTarget(appId)}
                  onBackToApps={clearAppTarget}
                  onWebCommand={handleAppWebCommand}
                  onSystemCommand={(appId, command) => void handleAppSystemCommand(appId, command)}
                  systemState={systemRunningState}
                  dockResult={systemDockResult}
                  needsRedock={systemNeedsRedock}
                  onBoundsChange={handleAppBoundsChange}
                />
              </ErrorBoundary>
            ) : (
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
                  onNavigate={setActiveView}
                  onOpenApp={(appId) => void openAppTarget(appId)}
                  iframeContainerRef={iframeContainerRef}
                >
                  {children}
                </MainContent>
              </ErrorBoundary>
            )}
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
                      className="flex items-center justify-center p-1.5 rounded-lg text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
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
            if (!hasNativeFiles()) throw new Error('file write failed: native file service unavailable');
            const fileName = filePath.split('/').pop() || filePath;
            // 字节级复制（copy_entry 自动建父目录 + 同名去重）；
            // 禁止 readFile+writeFileAtomic 文本中转——会损坏 PNG 二进制
            const result = await fsApi().copyEntry(filePath, `~/Desktop/\u7d20\u6750/${fileName}`);
            if (!result.ok) throw new Error(`file write failed: ${result.error || 'copy failed'}`);
            toast(t(locale, 'screenshot.saved'), 'success');
            return true;
          } catch (cause) {
            toast(classifyError(cause, { locale }).userMessage, 'error');
            return false;
          }
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
                return true;
              }
            }
            if (hasNativeFiles()) {
              const result = await fsApi().readFile(filePath) as string | { content?: string; encoding?: string };
              if (typeof result !== 'string' && result?.content && result?.encoding === 'base64') {
                const ext = (filePath.split('.').pop() || 'png').toLowerCase();
                const mime = ext === 'jpg' || ext === 'jpeg' ? 'image/jpeg' : 'image/png';
                setAnnotationImageUrl(`data:${mime};base64,${result.content}`);
                return true;
              }
            }
            throw new Error('file read failed: screenshot returned no image data');
          } catch (cause) {
            toast(classifyError(cause, { locale }).userMessage, 'error');
            return false;
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
            const api = window.nativesAPI;
            if (!api?.screenshot?.saveAnnotated) throw new Error('file write failed: screenshot service unavailable');
            await api.screenshot.saveAnnotated({ sourcePath: annotatingFile, dataUrl });
            toast(t(locale, 'screenshot.saved'), 'success');
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
