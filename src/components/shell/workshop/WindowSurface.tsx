'use client';

import React from 'react';
import AppBrowserPanel from '@/components/creative/AppBrowserPanel';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface WindowSurfaceProps {
  app: CreativeAppSummary;
  url: string;
  hostRef: React.RefObject<HTMLDivElement | null>;
  onStop: (app: CreativeAppSummary) => void;
  onRestart: (app: CreativeAppSummary) => void;
  onClose: () => void;
  onToast: (message: string) => void;
}

/** The single-window browser surface shown while an external app is open. */
export default function WindowSurface({
  app,
  url,
  hostRef,
  onStop,
  onRestart,
  onClose,
  onToast,
}: WindowSurfaceProps) {
  return (
    <AppBrowserPanel
      app={app}
      url={url}
      hostRef={hostRef}
      onStop={onStop}
      onRestart={onRestart}
      onClose={onClose}
      onToast={onToast}
    />
  );
}
