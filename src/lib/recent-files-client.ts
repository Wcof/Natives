'use client';

import { useState, useEffect, useCallback } from 'react';

/**
 * LRU 最近打开文件记录（客户端）
 *
 * 数据存储在 DB `settings:recent_files`，通过 IPC 读写。
 * 上限 30 个，新打开的文件推到队首，超出则淘汰最旧。
 */

const STORAGE_KEY = 'settings:recent_files';
const MAX_ENTRIES = 30;

export interface RecentFile {
  path: string;
  openedAt: number;
}

function readRaw(): string[] {
  // 同步读取缓存（首次由 hook 初始化）
  if (typeof window === 'undefined') return [];
  try {
    const raw = window.sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((x) => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

function writeRaw(paths: string[]): void {
  if (typeof window === 'undefined') return;
  try {
    window.sessionStorage.setItem(STORAGE_KEY, JSON.stringify(paths.slice(0, MAX_ENTRIES)));
    window.dispatchEvent(new CustomEvent('natives:recent-files-changed'));
  } catch {
    /* non-fatal */
  }
}

/** 从 DB 加载最近文件列表 */
async function loadFromDb(): Promise<string[]> {
  try {
    const raw = await window.nativesAPI?.db?.get(STORAGE_KEY);
    if (!raw) return [];
    const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
    return Array.isArray(parsed) ? parsed.filter((x) => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

/** 保存最近文件列表到 DB */
async function saveToDb(paths: string[]): Promise<void> {
  try {
    await window.nativesAPI?.db?.set(STORAGE_KEY, paths.slice(0, MAX_ENTRIES));
  } catch {
    /* non-fatal */
  }
}

/** 同步 sessionStorage 缓存与 DB */
async function syncCache(): Promise<void> {
  const dbPaths = await loadFromDb();
  writeRaw(dbPaths);
}

/**
 * 记录一个文件被打开
 * 推入 LRU 队列，异步持久化到 DB
 */
export async function pushRecentFile(filePath: string): Promise<void> {
  if (!filePath) return;

  // 从缓存读取作为 base，避免每次都要 await DB
  const paths = readRaw();
  const filtered = paths.filter((p) => p !== filePath);
  filtered.unshift(filePath);

  // 立即更新 sessionStorage（同步，无等待）
  writeRaw(filtered);

  // 异步持久化到 DB
  await saveToDb(filtered);
}

/**
 * 获取最近打开文件列表（同步，从 sessionStorage 缓存读取）
 */
export function getRecentFiles(limit = MAX_ENTRIES): string[] {
  return readRaw().slice(0, limit);
}

/**
 * React Hook：响应式返回最近打开文件列表
 * 首次挂载时从 DB 同步缓存
 */
export function useRecentFiles(limit = MAX_ENTRIES): {
  paths: string[];
  loading: boolean;
  refresh: () => void;
} {
  const [paths, setPaths] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(() => {
    setPaths(readRaw().slice(0, limit));
  }, [limit]);

  // 首次加载：从 DB 同步到缓存
  useEffect(() => {
    syncCache().then(() => {
      setPaths(readRaw().slice(0, limit));
      setLoading(false);
    });
  }, [limit]);

  // 监听变化事件（同 tab 跨组件同步）
  useEffect(() => {
    const onChange = () => {
      setPaths(readRaw().slice(0, limit));
    };
    window.addEventListener('natives:recent-files-changed', onChange);
    return () => window.removeEventListener('natives:recent-files-changed', onChange);
  }, [limit]);

  return { paths, loading, refresh };
}
