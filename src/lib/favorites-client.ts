'use client';

import { useState, useEffect, useCallback } from 'react';

/**
 * 全局收藏（侧边栏快捷入口）
 *
 * 与「快速访问」的区别：
 * - 快速访问：固定入口（主页 / 桌面 / …）
 * - 收藏：用户主动星标后才出现，可动态增减
 *
 * 当前生产者主要是文件管理器（kind=file）。
 * 数据模型预留 kind，便于以后收藏模块、视图、助理会话等。
 *
 * 存储：DB `settings:favorites`（JSON 字符串）
 * 同步：CustomEvent `favorites-changed`
 */

const STORAGE_KEY = 'settings:favorites';
const CHANGE_EVENT = 'favorites-changed';
const MAX_ENTRIES = 100;

/** 收藏领域；未知 kind 仍可导航（target 原样透传） */
export type FavoriteKind = 'file' | 'module' | 'view' | (string & {});

export interface FavoriteItem {
  /** 稳定主键：`${kind}:${target}` */
  id: string;
  kind: FavoriteKind;
  /** 导航目标：file=绝对路径；module=模块 id；view=视图 key */
  target: string;
  /** 列表展示名 */
  label: string;
  /** 仅 file：是否为目录（影响图标；缺省按目录处理） */
  isDir?: boolean;
  /** 可选图标 hint（未来 kind 用） */
  icon?: string;
  addedAt: number;
}

export interface ToggleFileFavoriteOptions {
  isDir?: boolean;
  label?: string;
}

function basename(path: string): string {
  if (!path) return path;
  const trimmed = path.replace(/\/+$/, '') || path;
  const parts = trimmed.split(/[/\\]/);
  return parts[parts.length - 1] || trimmed;
}

export function makeFavoriteId(kind: FavoriteKind, target: string): string {
  return `${kind}:${target}`;
}

/** 将任意历史/当前条目规范为 FavoriteItem；无法识别则返回 null */
export function normalizeFavorite(raw: unknown): FavoriteItem | null {
  if (typeof raw === 'string') {
    const path = raw.trim();
    if (!path) return null;
    return {
      id: makeFavoriteId('file', path),
      kind: 'file',
      target: path,
      label: basename(path),
      addedAt: Date.now(),
    };
  }

  if (!raw || typeof raw !== 'object') return null;
  const o = raw as Record<string, unknown>;

  // 新格式：{ kind, target, ... }
  if (typeof o.kind === 'string' && typeof o.target === 'string') {
    const kind = o.kind as FavoriteKind;
    const target = o.target;
    if (!target) return null;
    return {
      id: typeof o.id === 'string' && o.id ? o.id : makeFavoriteId(kind, target),
      kind,
      target,
      label: typeof o.label === 'string' && o.label ? o.label : basename(target),
      isDir: typeof o.isDir === 'boolean' ? o.isDir : undefined,
      icon: typeof o.icon === 'string' ? o.icon : undefined,
      addedAt: typeof o.addedAt === 'number' ? o.addedAt : Date.now(),
    };
  }

  // 旧格式：{ path, addedAt }
  if (typeof o.path === 'string' && o.path) {
    return {
      id: makeFavoriteId('file', o.path),
      kind: 'file',
      target: o.path,
      label: basename(o.path),
      addedAt: typeof o.addedAt === 'number' ? o.addedAt : Date.now(),
    };
  }

  return null;
}

/** 解析 DB/缓存中的收藏列表；去重、截断；按 addedAt 降序（新→旧） */
export function parseFavorites(raw: unknown): FavoriteItem[] {
  let parsed: unknown = raw;
  if (typeof raw === 'string') {
    try {
      parsed = JSON.parse(raw);
    } catch {
      return [];
    }
  }
  if (!Array.isArray(parsed)) return [];

  const items: FavoriteItem[] = [];
  const seen = new Set<string>();
  for (const entry of parsed) {
    const item = normalizeFavorite(entry);
    if (!item || seen.has(item.id)) continue;
    seen.add(item.id);
    items.push(item);
  }

  return items
    .sort((a, b) => b.addedAt - a.addedAt)
    .slice(0, MAX_ENTRIES);
}

export function isFavoritePath(items: FavoriteItem[], path: string): boolean {
  return items.some((f) => f.kind === 'file' && f.target === path);
}

export function favoriteFilePaths(items: FavoriteItem[]): string[] {
  return items.filter((f) => f.kind === 'file').map((f) => f.target);
}

/**
 * 切换文件路径收藏。返回 next 列表与本次是否为「加入」。
 * 纯函数，不落盘。
 */
export function toggleFileFavorite(
  items: FavoriteItem[],
  path: string,
  opts?: ToggleFileFavoriteOptions,
): { next: FavoriteItem[]; added: boolean } {
  if (!path) return { next: items, added: false };

  const existing = items.find((f) => f.kind === 'file' && f.target === path);
  if (existing) {
    return {
      next: items.filter((f) => f.id !== existing.id),
      added: false,
    };
  }

  const item: FavoriteItem = {
    id: makeFavoriteId('file', path),
    kind: 'file',
    target: path,
    label: opts?.label ?? basename(path),
    isDir: opts?.isDir,
    addedAt: Date.now(),
  };

  return {
    next: [item, ...items].slice(0, MAX_ENTRIES),
    added: true,
  };
}

/**
 * 通用移除（任意 kind）。纯函数。
 */
export function removeFavorite(
  items: FavoriteItem[],
  idOrKindTarget: { id: string } | { kind: FavoriteKind; target: string },
): FavoriteItem[] {
  if ('id' in idOrKindTarget) {
    return items.filter((f) => f.id !== idOrKindTarget.id);
  }
  const id = makeFavoriteId(idOrKindTarget.kind, idOrKindTarget.target);
  return items.filter((f) => f.id !== id);
}

/** 侧边栏 / 壳层导航 target 字符串 */
export function favoritesNavTarget(item: FavoriteItem): string {
  switch (item.kind) {
    case 'file':
      return `__files__:${item.target}`;
    case 'module': {
      const t = item.target;
      if (t.startsWith('module:') || t.startsWith('builtin:')) return t;
      return `module:${t}`;
    }
    case 'view':
      return item.target;
    default:
      return item.target;
  }
}

export async function loadFavorites(): Promise<FavoriteItem[]> {
  try {
    const stored = await window.nativesAPI?.db?.get?.(STORAGE_KEY);
    if (stored == null || stored === '') return [];
    return parseFavorites(stored);
  } catch {
    return [];
  }
}

export async function saveFavorites(items: FavoriteItem[]): Promise<void> {
  const payload = JSON.stringify(items.slice(0, MAX_ENTRIES));
  try {
    await window.nativesAPI?.db?.set?.(STORAGE_KEY, payload);
  } catch {
    /* non-fatal: still broadcast so in-memory peers refresh */
  }
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent(CHANGE_EVENT));
  }
}

/**
 * 切换文件收藏并持久化。
 * 返回更新后的列表与是否加入。
 */
export async function toggleAndPersistFileFavorite(
  path: string,
  opts?: ToggleFileFavoriteOptions,
): Promise<{ next: FavoriteItem[]; added: boolean }> {
  const current = await loadFavorites();
  const { next, added } = toggleFileFavorite(current, path, opts);
  await saveFavorites(next);
  return { next, added };
}

/**
 * 移除并持久化（侧边栏取消收藏等）。
 */
export async function removeAndPersistFavorite(
  idOrKindTarget: { id: string } | { kind: FavoriteKind; target: string },
): Promise<FavoriteItem[]> {
  const current = await loadFavorites();
  const next = removeFavorite(current, idOrKindTarget);
  await saveFavorites(next);
  return next;
}

export function useFavorites(): {
  favorites: FavoriteItem[];
  loading: boolean;
  refresh: () => void;
} {
  const [favorites, setFavorites] = useState<FavoriteItem[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(() => {
    void loadFavorites().then((items) => {
      setFavorites(items);
      setLoading(false);
    });
  }, []);

  useEffect(() => {
    refresh();
    const handler = () => refresh();
    window.addEventListener(CHANGE_EVENT, handler);
    return () => window.removeEventListener(CHANGE_EVENT, handler);
  }, [refresh]);

  return { favorites, loading, refresh };
}

export const FAVORITES_STORAGE_KEY = STORAGE_KEY;
export const FAVORITES_CHANGE_EVENT = CHANGE_EVENT;
export const FAVORITES_MAX_ENTRIES = MAX_ENTRIES;
/** 侧边栏默认展示条数，超出可展开 */
export const FAVORITES_SIDEBAR_PREVIEW = 5;
