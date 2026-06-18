'use client';

import { useState, useEffect, useCallback } from 'react';

/**
 * LRU 最近打开文件记录（客户端）
 *
 * 数据存储在 DB `settings:recent_files`，通过 IPC 读写。
 * 缓存使用 localStorage（跨 tab 持久化），上限 30 个。
 * 使用 Promise 链序列化写入，防止并发竞争。
 */

const STORAGE_KEY = 'settings:recent_files';
const MAX_ENTRIES = 30;

export interface RecentFile {
  path: string;
  openedAt: number;
}

// ── 写入序列化（防止并发竞争） ──

let writeChain: Promise<void> = Promise.resolve();

function readRaw(): string[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
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
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(paths.slice(0, MAX_ENTRIES)));
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

/** 同步 localStorage 缓存与 DB */
async function syncCache(): Promise<void> {
  const dbPaths = await loadFromDb();
  writeRaw(dbPaths);
}

/**
 * 记录一个文件被打开
 * 使用 Promise 链序列化，防止并发写入导致数据丢失
 */
export function pushRecentFile(filePath: string): Promise<void> {
  if (!filePath) return Promise.resolve();

  writeChain = writeChain.then(async () => {
    // 从缓存读取最新状态（可能已被前一个 promise 更新）
    const paths = readRaw();
    const filtered = paths.filter((p) => p !== filePath);
    filtered.unshift(filePath);

    // 立即更新 localStorage（同步）
    writeRaw(filtered);

    // 异步持久化到 DB
    await saveToDb(filtered);
  });

  return writeChain;
}

/**
 * 获取最近打开文件列表（同步，从 localStorage 缓存读取）
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
  const [paths, setPaths] = useState<string[]>(() => readRaw().slice(0, limit));
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
    // 跨 tab 同步（localStorage 的 storage 事件只在其他 tab 触发）
    window.addEventListener('storage', onChange);
    return () => {
      window.removeEventListener('natives:recent-files-changed', onChange);
      window.removeEventListener('storage', onChange);
    };
  }, [limit]);

  return { paths, loading, refresh };
}
