import type { RightPanelMode } from '../RightPanel';

import { useEffect } from 'react';
import type { FileEntry } from '@/types/file';

interface UseFileEventsOptions {
  followMode: string;
  terminalSessionIdRef: React.RefObject<string | null>;
  selectedFile: FileEntry | null;
  setSelectedFile: (file: FileEntry | null) => void;
  setRightPanelMode: (mode: RightPanelMode) => void;
}

export function useFileEvents({
  followMode,
  terminalSessionIdRef,
  selectedFile,
  setSelectedFile,
  setRightPanelMode,
}: UseFileEventsOptions) {
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
        api.terminal.write(sessionId, `cd "${dirPath}"\r`);
      }
    };
    window.addEventListener('navigate-files', handler);
    return () => window.removeEventListener('navigate-files', handler);
  }, [followMode, terminalSessionIdRef]);

  // Shell:focus-base from iframes (Cmd+Shift+K)
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

  // Sync preview when file is renamed or trashed
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
  }, [selectedFile, setSelectedFile, setRightPanelMode]);
}
