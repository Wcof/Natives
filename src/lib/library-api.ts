/**
 * Library 域契约与 IPC 访问封装（Hub 面）。
 *
 * - 类型与 src-tauri/src/commands/library.rs 的 serde(camelCase) 输出对齐；
 *   此前 6 个组件各自内联声明 interface 已出现字段漂移，此文件是唯一来源。
 * - 访问方式对齐 files-api / jobs-api：组件不得直接摸 window.nativesAPI.library，
 *   一律经 libraryApiOrNull()（浏览器 dev 环境返回 null，由调用方降级）。
 */

export interface LibraryFolder {
  id: string;
  name: string;
  parentId: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface LibraryTag {
  id: string;
  name: string;
  color: string;
  createdAt: string;
}

export interface LibraryItem {
  id: string;
  folderId: string | null;
  title: string;
  description: string;
  content: string;
  sourceUrl: string;
  itemType: string;
  status: string;
  createdAt: string;
  updatedAt: string;
  tags: LibraryTag[];
}

export interface LibraryStats {
  totalItems: number;
  totalFolders: number;
  totalTags: number;
  itemsByFolder: { folderId: string | null; folderName: string; count: number }[];
  recentItems: number;
}

export interface LibraryItemFilter {
  folderId?: string;
  tagId?: string;
  keyword?: string;
  status?: string;
  itemType?: string;
  limit?: number;
  offset?: number;
}

export interface LibraryCreateItemInput {
  folderId?: string;
  title: string;
  description?: string;
  content?: string;
  sourceUrl?: string;
  itemType?: string;
  status?: string;
  tagIds?: string[];
}

/** 部分更新：缺字段 = 后端保留现值；folderId 显式 null = 移出文件夹 */
export interface LibraryUpdateItemInput {
  id: string;
  folderId?: string | null;
  title?: string;
  description?: string;
  content?: string;
  sourceUrl?: string;
  status?: string;
  tagIds?: string[];
}

export interface LibraryApi {
  listFolders: () => Promise<LibraryFolder[]>;
  createFolder: (data: { name: string; parentId?: string }) => Promise<LibraryFolder>;
  updateFolder: (data: { id: string; name: string }) => Promise<void>;
  deleteFolder: (id: string, moveItems: boolean) => Promise<void>;
  listTags: () => Promise<LibraryTag[]>;
  createTag: (data: { name: string; color: string }) => Promise<LibraryTag>;
  deleteTag: (id: string) => Promise<void>;
  listItems: (filter: LibraryItemFilter) => Promise<LibraryItem[]>;
  getItem: (id: string) => Promise<LibraryItem | null>;
  createItem: (data: LibraryCreateItemInput) => Promise<LibraryItem>;
  updateItem: (data: LibraryUpdateItemInput) => Promise<void>;
  deleteItem: (id: string) => Promise<void>;
  batchTag: (data: { itemIds: string[]; tagIds: string[] }) => Promise<void>;
  batchMove: (data: { itemIds: string[]; folderId?: string }) => Promise<void>;
  batchDelete: (data: { itemIds: string[] }) => Promise<void>;
  getStats: () => Promise<LibraryStats>;
}

/** Tauri 环境返回强类型 Library API；浏览器 dev 环境返回 null（调用方降级展示错误态） */
export function libraryApiOrNull(): LibraryApi | null {
  if (typeof window === 'undefined') return null;
  const api = window.nativesAPI?.library;
  return (api as unknown as LibraryApi) ?? null;
}

/** 后端 item_type 值域（DB default 'note'）；枚举展示必须走 i18n（library.itemType.*） */
export const ITEM_TYPES = ['note', 'link', 'file', 'code', 'image', 'bookmark'] as const;
export type ItemType = (typeof ITEM_TYPES)[number];

/** 后端 status 值域（DB default 'active'）；枚举展示必须走 i18n（library.statusValue.*） */
export const ITEM_STATUSES = ['active', 'archived'] as const;

/** 单页条数 — 与后端 library_list_items 默认 LIMIT 对齐（上限 200） */
export const LIBRARY_PAGE_SIZE = 50;

/** 预置标签色板（写库的数据值，非 UI 样式；展示用作 border-left 色条） */
export const TAG_COLOR_PRESETS = [
  '#6366f1', '#f97316', '#10b981', '#ef4444', '#eab308', '#8b5cf6', '#0ea5e9', '#64748b',
] as const;
