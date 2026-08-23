'use client';

import WorkspaceCompositionPage from '@/components/workspace/WorkspaceCompositionPage';
import '@/types';

/**
 * `/` —— Personal Workspace Home (V2).
 * Local-First Multi-Workspace composition surface supporting compact grid,
 * free canvas, data views, and inspector.
 */
export default function DashboardPage() {
  return (
    <div className="w-full h-full flex flex-col overflow-hidden">
      <WorkspaceCompositionPage />
    </div>
  );
}
