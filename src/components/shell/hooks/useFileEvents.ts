import type { RightPanelMode } from '../RightPanel';

import { useEffect } from 'react';
import type { FileEntry } from '@/types/file';
import { FILE_EVENTS, onFileEvent } from '@/lib/file-events';

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
    // 订阅文件域导航事件（file-events 契约）；仅响应字符串形式的绝对路径
    return onFileEvent(FILE_EVENTS.navigateFiles, (payload) => {
      if (typeof payload !== 'string' || !payload.startsWith('/')) return;
      const sessionId = terminalSessionIdRef.current;
      if (!sessionId) return;
      const api = window.nativesAPI;
      if (api?.terminal?.write) {
        api.terminal.write(sessionId, `cd "${payload}"\r`);
      }
    });
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

  // 重命名 / 移入废纸篓后同步预览面板（file-events 契约）
  useEffect(() => {
    const offRenamed = onFileEvent(FILE_EVENTS.fileRenamed, ({ oldPath, newPath }) => {
      if (!selectedFile || !oldPath || !newPath) return;
      if (selectedFile.path === oldPath) {
        const newName = newPath.split('/').pop() || selectedFile.name;
        setSelectedFile({ ...selectedFile, path: newPath, name: newName });
      }
    });
    const offTrashed = onFileEvent(FILE_EVENTS.fileTrashed, ({ path }) => {
      if (selectedFile && selectedFile.path === path) {
        setSelectedFile(null);
        setRightPanelMode('closed');
      }
    });
    return () => {
      offRenamed();
      offTrashed();
    };
  }, [selectedFile, setSelectedFile, setRightPanelMode]);
}
