'use client';

import { lazy, Suspense, useEffect } from 'react';
import { type Locale } from '@/i18n';
import type { FileEntry } from '@/types/file';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { getBuiltinTool } from '@/lib/builtin-tools';
import { isSettingsView, getSettingsSection } from './settings-navigation';

// Lazy-loaded heavy page components
const LazyAppsPage = lazy(() => import('@/components/apps/AppsPage'));
const LazyFileBrowser = lazy(() => import('@/components/files/FileBrowser'));
const LazyAiWorkbench = lazy(() => import('@/components/ai/AiWorkbench'));
const LazyUsageDashboard = lazy(() => import('@/components/dashboard/UsageDashboard').then((m) => ({ default: m.UsageDashboard })));
const LazySettingsPage = lazy(() => import('./SettingsPage'));

const BUILTIN_LAZY_MAP: Record<string, React.LazyExoticComponent<React.ComponentType>> = {};

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
  onNavigate: (view: string) => void;
  onOpenApp: (appId: string) => void;
  children: React.ReactNode;
  iframeContainerRef: React.RefObject<HTMLDivElement | null>;
}

export default function MainContent({
  activeView,
  locale,
  onFileSelect,
  onNavigate,
  onOpenApp,
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
    return <Suspense fallback={<LazyFallback />}><LazySettingsPage activeSection={getSettingsSection(activeView)} locale={locale} onNavigate={onNavigate} /></Suspense>;
  }

  switch (activeView) {
    case 'apps':
    case 'workshop':
    case 'modules':
    case 'store':
      return (
        <Suspense fallback={<LazyFallback />}>
          <LazyAppsPage onOpenApp={onOpenApp} />
        </Suspense>
      );
    case 'files':
    case 'library':
      return (
        <Suspense fallback={<LazyFallback />}>
          <LazyFileBrowser onFileSelect={onFileSelect} />
        </Suspense>
      );
    case 'ai':
    case 'assistant':
    case 'capabilities':
    case 'tools':
      return (
        <Suspense fallback={<LazyFallback />}>
          <LazyAiWorkbench />
        </Suspense>
      );
    case 'dashboard':
      return children;
    case 'usage':
    case 'jobs':
      return (
        <Suspense fallback={<LazyFallback />}>
          <LazyUsageDashboard />
        </Suspense>
      );
    default:
      if (activeView.startsWith('module:')) {
        return <div ref={iframeContainerRef} style={{ width: '100%', height: '100%' }} />;
      }
      return children;
  }
}
