'use client';

import { lazy, Suspense, useEffect } from 'react';
import { type Locale } from '@/i18n';
import type { FileEntry } from '@/types/file';
import ErrorBoundary from '@/components/ui/ErrorBoundary';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { getBuiltinTool } from '@/lib/builtin-tools';
import { isSettingsView, getSettingsSection } from './settings-navigation';

// Lazy-loaded heavy page components
const LazyWorkshopPage = lazy(() => import('./WorkshopPage'));
const LazyFileBrowser = lazy(() => import('@/components/files/FileBrowser'));
const LazyFilePreview = lazy(() => import('@/components/files/FilePreview'));
const LazyAiWorkbench = lazy(() => import('@/components/ai/AiWorkbench'));
const LazyToolsPage = lazy(() => import('@/components/tools/ToolsPage'));
const LazyAssistantWorkbench = lazy(() => import('@/components/assistant/AssistantWorkbench'));
const LazyJobsPage = lazy(() => import('@/components/jobs/JobsPage'));
const LazyCapabilitiesPage = lazy(() => import('@/components/capabilities/CapabilitiesPage'));
const LazySettingsPage = lazy(() => import('./SettingsPage'));

const BUILTIN_LAZY_MAP: Record<string, React.LazyExoticComponent<any>> = {};

const LazyFallback = () => (
  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', gap: 16 }}>
    <MathCurveLoader size={48} />
  </div>
);

function BuiltinToolLauncher({ toolId, children }: { toolId: string; children: React.ReactNode }) {
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await window.nativesAPI?.builtinTool?.list?.();
        if (cancelled) return;
        const entry = list?.find((t: { id: string }) => t.id === toolId);
        if (entry?.driver && entry.driver !== 'native') await window.nativesAPI?.builtinTool?.launch?.(entry.driver);
      } catch { /* best effort */ }
    })();
    return () => { cancelled = true; };
  }, [toolId]);
  return <>{children}</>;
}

export interface MainContentProps {
  activeView: string;
  locale: Locale;
  httpPort: number | null;
  selectedFile: FileEntry | null;
  setSelectedFile: (file: FileEntry | null) => void;
  editMode: boolean;
  setEditMode: (mode: boolean) => void;
  iframeReloadKey: number;
  terminalSessionId: string | null;
  onFileSelect: (file: FileEntry) => void;
  children: React.ReactNode;
  iframeContainerRef: React.RefObject<HTMLDivElement | null>;
}

export default function MainContent({
  activeView,
  locale,
  httpPort,
  selectedFile,
  setSelectedFile,
  editMode,
  setEditMode,
  iframeReloadKey,
  terminalSessionId,
  onFileSelect,
  children,
  iframeContainerRef,
}: MainContentProps) {
  // Builtin tool routing: builtin:terminal → open bottom panel, others → lazy content
  if (activeView.startsWith('builtin:')) {
    const toolId = activeView.slice('builtin:'.length);
    const toolDef = getBuiltinTool(toolId);

    if (toolId === 'terminal') {
      return children;
    }

    if (toolDef?.componentPath && toolDef.displayMode === 'content') {
      const LazyComponent = BUILTIN_LAZY_MAP[toolDef.id];
      if (LazyComponent) {
        return (
          <Suspense fallback={<LazyFallback />}>
            <LazyComponent />
          </Suspense>
        );
      }
      return <div style={{ padding: 40, color: 'var(--text-secondary)' }}>Component not registered: {toolDef.componentPath}</div>;
    }

    if (toolDef) {
      return <BuiltinToolLauncher toolId={toolId}>{children}</BuiltinToolLauncher>;
    }

    return children;
  }

  // Settings routing — handle all settings: prefixed views
  if (isSettingsView(activeView)) {
    return <Suspense fallback={<LazyFallback />}><LazySettingsPage activeSection={getSettingsSection(activeView)} locale={locale} /></Suspense>;
  }

  switch (activeView) {
    case 'workshop':
    case 'modules':
    case 'store':
      // Single surface: Personal Creations (install + create + manage)
      return <Suspense fallback={<LazyFallback />}><LazyWorkshopPage onInstall={() => {}} /></Suspense>;
    case 'files':
      return (
        <Suspense fallback={<LazyFallback />}>
          <LazyFileBrowser onFileSelect={onFileSelect} />
        </Suspense>
      );
    case 'ai':
      return <Suspense fallback={<LazyFallback />}><LazyAiWorkbench /></Suspense>;
    case 'assistant':
      return <Suspense fallback={<LazyFallback />}><LazyAssistantWorkbench locale={locale} /></Suspense>;
    case 'jobs':
      return <Suspense fallback={<LazyFallback />}><LazyJobsPage /></Suspense>;
    case 'capabilities':
      return <Suspense fallback={<LazyFallback />}><LazyCapabilitiesPage /></Suspense>;
    case 'tools':
      return <Suspense fallback={<LazyFallback />}><LazyToolsPage /></Suspense>;
    case 'dashboard':
      return children;
    default:
      if (activeView.startsWith('module:')) {
        return <div ref={iframeContainerRef} style={{ width: '100%', height: '100%' }} />;
      }
      return children;
  }
}
