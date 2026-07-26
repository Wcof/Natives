'use client';

/**
 * file-events — 文件域跨组件事件的唯一契约
 *
 * 此前 `navigate-files` / `header-file-action` 等 7 个事件名与 payload
 * 结构散落在 FileBrowser / Header / Sidebar / Terminal / Assistant 各处，
 * 以裸字符串 + 无类型 detail 传递（隐性契约），另有两个补丁式
 * `window.__pending*` 全局量。本模块把它们收敛为：
 * - 事件名常量 + payload 类型映射（编译期对齐两端）
 * - 类型安全的 dispatch / on 封装
 * - 挂载竞态的 pending 状态（替代 window 全局量）
 *
 * 新增文件域跨组件事件必须在这里登记，禁止再裸写字符串。
 */

// ── 事件名常量 ──

export const FILE_EVENTS = {
  /** 请求文件浏览器跳转（Sidebar / CommandPalette / Terminal / Assistant → FileBrowser） */
  navigateFiles: 'navigate-files',
  /** Header 工具条动作下行（Header → FileBrowser） */
  headerFileAction: 'header-file-action',
  /** 文件浏览器状态上行广播（FileBrowser → Header） */
  headerFileState: 'header-file-state',
  /** 重命名完成（FileBrowser → 预览面板同步标题） */
  fileRenamed: 'file-renamed',
  /** 移入废纸篓完成（FileBrowser → 预览面板自动关闭） */
  fileTrashed: 'file-trashed',
  /** 点亮某个条目卡片（fs_watch / duplicate → FileCard heat） */
  fileFlash: 'file-flash',
  /** 展开终端面板（幂等：仅在折叠时展开） */
  openTerminal: 'open-terminal',
} as const;

export type FileEventName = (typeof FILE_EVENTS)[keyof typeof FILE_EVENTS];

// ── payload 契约 ──

export type FileSortField = 'name' | 'mtime' | 'size';

/** Header 下行动作（discriminated union，新动作在此登记） */
export type HeaderFileAction =
  | { type: 'viewMode'; value: 'grid' | 'list' }
  | { type: 'sortBy'; value: FileSortField }
  | { type: 'sortDir'; value?: 'asc' | 'desc' }
  | { type: 'showHidden' }
  | { type: 'search'; value?: string }
  | { type: 'newFolder'; value?: string }
  | { type: 'newFile'; value?: string }
  | { type: 'gridSize'; value: 'sm' | 'md' | 'lg' }
  | { type: 'back' }
  | { type: 'forward' }
  | { type: 'up' }
  | { type: 'refresh' }
  | { type: 'toggleRecent' }
  | { type: 'toggleFavorite' }
  | { type: 'globalSearch' }
  | { type: 'goToPath'; value: string };

/** FileBrowser 上行状态广播 */
export interface HeaderFileState {
  viewMode: 'grid' | 'list';
  sortBy: FileSortField;
  sortDir: 'asc' | 'desc';
  showHidden: boolean;
  gridSize: 'sm' | 'md' | 'lg';
  segments: string[];
  isFavorite: boolean;
  breadcrumbPath: string;
  projectBadge: string | null;
  canGoBack: boolean;
  canGoForward: boolean;
  canGoUp: boolean;
  recentMode: boolean;
  recentOpenedMode: boolean;
  searchQuery: string;
  loading: boolean;
}

export type NavigateFilesPayload = string | { path?: string; directory?: string };

export interface FileEventPayloads {
  'navigate-files': NavigateFilesPayload;
  'header-file-action': HeaderFileAction;
  'header-file-state': HeaderFileState;
  'file-renamed': { oldPath: string; newPath: string };
  'file-trashed': { path: string };
  'file-flash': string;
  'open-terminal': undefined;
}

// ── 类型安全的 dispatch / listen ──

export function dispatchFileEvent<K extends FileEventName>(
  name: K,
  ...payload: FileEventPayloads[K] extends undefined ? [] : [FileEventPayloads[K]]
): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent(name, { detail: payload[0] }));
}

/** 订阅文件域事件，返回取消函数 */
export function onFileEvent<K extends FileEventName>(
  name: K,
  handler: (payload: FileEventPayloads[K]) => void,
): () => void {
  if (typeof window === 'undefined') return () => {};
  const listener = (e: Event) => handler((e as CustomEvent<FileEventPayloads[K]>).detail);
  window.addEventListener(name, listener);
  return () => window.removeEventListener(name, listener);
}

// ── 挂载竞态的 pending 状态（替代 window.__pendingNavigateFiles / __pendingSelectFile）──
//
// 场景：Sidebar 等在 FileBrowser 尚未挂载（或刚切走）时 dispatch 跳转，
// 事件无人接听。发起方 setPendingNavigate 兜底，FileBrowser 挂载时 consume。

let pendingNavigate: string | null = null;
let pendingSelectFile: string | null = null;

export function setPendingNavigate(path: string): void {
  pendingNavigate = path;
}

export function consumePendingNavigate(): string | null {
  const v = pendingNavigate;
  pendingNavigate = null;
  return v;
}

export function setPendingSelectFile(path: string): void {
  pendingSelectFile = path;
}

export function consumePendingSelectFile(): string | null {
  const v = pendingSelectFile;
  pendingSelectFile = null;
  return v;
}

/** 便捷入口：跳转到某路径（自动带 pending 兜底），全项目统一用它 */
export function navigateToFiles(path: string): void {
  setPendingNavigate(path);
  dispatchFileEvent(FILE_EVENTS.navigateFiles, path);
}
