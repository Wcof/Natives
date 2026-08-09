'use client';

/**
 * useFileFavorites — FileBrowser 收藏状态（当前目录星标）。
 *
 * 职责边界（F3-04 拆分，ARCH-002）：
 * - favorites 列表的加载 / favorites-changed 事件同步
 * - 当前路径是否收藏（isFavorite）+ 全部收藏路径（favoritePaths，供卡片星标）
 * - toggleFavorite：切换星标并持久化，成功/失败 toast
 *
 * 不持有：导航、entries 加载、选择、文件操作。依赖 currentPath / entries
 * 推断目标是否为目录（与导航、entries 挂钩由 FileBrowser 组合）。
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import {
  type FavoriteItem,
  favoriteFilePaths,
  isFavoritePath,
  loadFavorites,
  saveFavorites,
  toggleFileFavorite,
} from '@/lib/favorites-client';

export interface UseFileFavoritesOptions {
  currentPath: string;
  /** 用于从路径反查条目推断 isDir；无匹配时按目录处理 */
  entries: FileEntry[];
  showToast: (msg: string) => void;
  locale: Locale;
}

export interface UseFileFavoritesResult {
  favorites: FavoriteItem[];
  /** 当前目录是否已收藏 */
  isFavorite: boolean;
  /** 全部 file 收藏路径（供 grid/list 星标渲染） */
  favoritePaths: string[];
  toggleFavorite: (targetPath?: string) => Promise<void>;
  handleFavoriteToggle: (entry: FileEntry) => void;
}

export function useFileFavorites({
  currentPath,
  entries,
  showToast,
  locale,
}: UseFileFavoritesOptions): UseFileFavoritesResult {
  const [favorites, setFavorites] = useState<FavoriteItem[]>([]);

  // Load favorites + keep in sync with favorites-changed (sidebar 等其它写入方)
  useEffect(() => {
    async function load() {
      try {
        const items = await loadFavorites();
        setFavorites(items);
      } catch {
        /* ignore */
      }
    }
    void load();

    const onFavoritesChanged = () => {
      void loadFavorites().then(setFavorites);
    };
    window.addEventListener('favorites-changed', onFavoritesChanged);
    return () => window.removeEventListener('favorites-changed', onFavoritesChanged);
  }, []);

  const isFavorite = isFavoritePath(favorites, currentPath);

  const toggleFavorite = useCallback(
    async (targetPath?: string) => {
      const path = targetPath ?? currentPath;
      if (!path) return;

      // Infer isDir from current entries / current folder when available
      let isDir: boolean | undefined;
      if (path === currentPath) {
        isDir = true;
      } else {
        const entry = entries.find((e) => e.path === path);
        if (entry) isDir = entry.isDir;
      }

      const { next, added } = toggleFileFavorite(favorites, path, {
        isDir,
        label: path.split(/[/\\]/).pop() || path,
      });
      setFavorites(next);
      try {
        await saveFavorites(next);
        showToast(added ? t(locale, 'fileBrowser.addedToFavorites') : t(locale, 'fileBrowser.removedFromFavorites'));
      } catch {
        /* ignore */
      }
    },
    [currentPath, favorites, entries, showToast, locale],
  );

  const favoritePaths = useMemo(() => favoriteFilePaths(favorites), [favorites]);

  const handleFavoriteToggle = useCallback(
    (entry: FileEntry) => {
      void toggleFavorite(entry.path);
    },
    [toggleFavorite],
  );

  return { favorites, isFavorite, favoritePaths, toggleFavorite, handleFavoriteToggle };
}
