'use client';

import { useState, useCallback, useRef } from 'react';

interface UseFileDropOptions {
  /** Called with real filesystem paths (Electron file drops) */
  onFilesDropped?: (filePaths: string[]) => void;
  /** Called with saved file info after URL/image drops */
  onUrlDropped?: (savedPath: string) => void;
  /** Current directory for saving dropped URLs */
  currentDir?: string;
}

/**
 * Unified drag-and-drop hook.
 *
 * Supports two drag payloads:
 *  1. System files (Finder) → resolves `file.path` and calls onFilesDropped
 *  2. text/uri-list (WeChat images, browser <img> drags) → fetches the URL,
 *     saves to currentDir via /api/fs/save, calls onUrlDropped
 *
 * The drop zone covers the entire bound element — empty space below files is
 * included, so "下半截拖不进" is fixed.
 */
export function useFileDrop({ onFilesDropped, onUrlDropped, currentDir }: UseFileDropOptions) {
  const [isDragging, setIsDragging] = useState(false);
  const dragCounter = useRef(0);

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounter.current++;
    const types = e.dataTransfer?.types;
    if (types && (types.includes('Files') || types.includes('text/uri-list'))) {
      setIsDragging(true);
    }
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounter.current--;
    if (dragCounter.current === 0) {
      setIsDragging(false);
    }
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    // Required to allow drop
    e.dataTransfer.dropEffect = 'copy';
  }, []);

  const handleDrop = useCallback(async (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
    dragCounter.current = 0;

    const dt = e.dataTransfer;
    if (!dt) return;

    // 1. Real files from Finder
    const files = Array.from(dt.files || []);
    if (files.length > 0) {
      const paths = files.map((f) => (f as any).path).filter(Boolean);
      if (paths.length > 0 && onFilesDropped) {
        onFilesDropped(paths);
        return;
      }
    }

    // 2. text/uri-list (images dragged from WeChat, browser, etc.)
    if (dt.types.includes('text/uri-list') && currentDir) {
      const raw = dt.getData('text/uri-list');
      if (raw) {
        // Parse uri-list: first non-comment, non-empty line
        const url = raw.split(/[\r\n]/).find((l) => l && !l.trim().startsWith('#'))?.trim();
        if (url) {
          try {
            await dropUrlInto(url, currentDir, onUrlDropped);
          } catch (err) {
            console.error('[useFileDrop] Failed to save dropped URL:', err);
          }
          return;
        }
      }
    }
  }, [onFilesDropped, onUrlDropped, currentDir]);

  return {
    isDragging,
    dragHandlers: {
      onDragEnter: handleDragEnter,
      onDragLeave: handleDragLeave,
      onDragOver: handleDragOver,
      onDrop: handleDrop,
    },
  };
}

/**
 * Fetch an image URL and save it to the target directory.
 * Mirrors fanbox's dropUrlInto pattern.
 */
async function dropUrlInto(
  url: string,
  dir: string,
  onSaved?: (path: string) => void,
): Promise<void> {
  const resp = await fetch(url);
  if (!resp.ok) throw new Error(`Fetch failed: ${resp.status}`);

  const blob = await resp.blob();

  // Only accept images
  if (!/^image\//.test(blob.type)) {
    console.warn('[useFileDrop] Non-image URL dropped, ignoring:', blob.type);
    return;
  }

  // Derive extension from MIME
  const ext = (blob.type.split('/')[1] || 'png')
    .toLowerCase()
    .replace('jpeg', 'jpg')
    .replace(/[^a-z0-9]/g, '') || 'png';

  // Extract filename from URL, or generate one
  let name: string;
  try {
    const urlPath = new URL(url, location.href).pathname;
    const base = urlPath.split('/').pop() || '';
    name = decodeURIComponent(base);
    if (!name || !/\.[a-z0-9]+$/i.test(name)) {
      name = `image-${Date.now()}.${ext}`;
    }
  } catch {
    name = `image-${Date.now()}.${ext}`;
  }

  // POST binary data to /api/fs/save
  const port = (window as any).__nativesHttpPort || 3000;
  const arrayBuf = await blob.arrayBuffer();
  const saveResp = await fetch(
    `http://localhost:${port}/api/fs/save?dir=${encodeURIComponent(dir)}&name=${encodeURIComponent(name)}`,
    {
      method: 'POST',
      body: arrayBuf,
    },
  );

  if (!saveResp.ok) {
    const err = await saveResp.text();
    throw new Error(`Save failed: ${err}`);
  }

  const result = await saveResp.json();
  if (result?.path && onSaved) {
    onSaved(result.path);
  }
}
