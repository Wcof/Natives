'use client';

import { lazy, Suspense } from 'react';
import { type Locale } from '@/i18n';
import type { FileEntry } from '@/types/file';
import ErrorBoundary from '@/components/ui/ErrorBoundary';
import SettingsPage from './SettingsPage';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { getBuiltinTool } from '@/lib/builtin-tools';

// Lazy-loaded heavy page components
const LazyWorkshopPage = lazy(() => import('./WorkshopPage'));
const LazyFileBrowser = lazy(() => import('@/components/files/FileBrowser'));
const LazyFilePreview = lazy(() => import('@/components/files/FilePreview'));
const LazyAiWorkbench = lazy(() => import('@/components/ai/AiWorkbench'));
const LazyToolsPage = lazy(() => import('@/components/tools/ToolsPage'));
const LazyModulesPage = lazy(() => import('@/app/modules/page'));

const BUILTIN_LAZY_MAP: Record<string, React.LazyExoticComponent<any>> = {};

const LazyFallback = () => (
  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', gap: 16 }}>
    <MathCurveLoader size={48} />
  </div>
);

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
      return <div style={{ padding: 40, color: 'var(--text-dim)' }}>Component not registered: {toolDef.componentPath}</div>;
    }

    if (toolDef) {
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
      return <Suspense fallback={<LazyFallback />}><LazyWorkshopPage onInstall={() => {}} /></Suspense>;
    case 'files':
      return (
        <Suspense fallback={<LazyFallback />}>
          <LazyFileBrowser onFileSelect={onFileSelect} />
        </Suspense>
      );
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
        return <div ref={iframeContainerRef} style={{ width: '100%', height: '100%' }} />;
      }
      return children;
  }
}
