// ── File Types ──
//
// 文件域的 wire 类型（FileEntry/StatResult/…）由 Rust 单一来源生成
// （src-tauri/src/file_manager.rs 等 → `npm run types:generate` → src/types/generated/），
// 本文件只做 re-export + 前端自有的视图模型。
// 禁止在前端重新手写 wire 结构或重复实现 kind/项目徽章探测——
// 那正是曾经导致双端漂移（TS 与 Rust 扩展名表各自演化）的根源。

export type {
  FileEntry,
  StatResult,
  ListDirOptions,
  ListDirResult,
  ReadFileResult,
  WriteResult,
  FsWatchEvent,
  ArchiveEntry,
  ArchiveListing,
  DiskUsageItem,
} from './generated';

import type { FileEntry } from './generated';

/** 文件类型分类（值域随 Rust detect_file_kind 生成，含目录 "dir"） */
export type FileKind = FileEntry['kind'];

/** 项目徽章类型（值域随 Rust detect_project_badge 生成） */
export type ProjectBadge = NonNullable<FileEntry['projectBadge']>;

// ── 前端自有的视图模型（非 wire 契约）──

/** 搜索结果（search-engine.ts 内部引擎的归一化形态；wire 形态见 generated/SearchResult） */
export interface SearchResult {
  path: string;
  name: string;
  score: number;
  isDir: boolean;
  mtime: number;
  matchRanges: [number, number][];
}

/** 内容搜索结果（FileSearch 面板的归一化形态） */
export interface ContentSearchResult {
  path: string;
  name: string;
  line: number;
  preview: string;
  matchStart: number;
  matchEnd: number;
  score?: number;
  mtime?: number;
}

/** Git 状态 */
export interface GitStatus {
  root: string;
  branch: string;
  files: GitFileStatus[];
}

/** Git 文件状态 */
export interface GitFileStatus {
  path: string;
  status: 'M' | 'A' | 'D' | 'R' | '??' | 'UU';
  oldPath?: string;
}

/** 单模型统计（用于 usage.refresh() 响应） */
export interface ModelStat {
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens?: number;
  cacheReadTokens?: number;
  requestCount: number;
  totalTokens: number;
  totalCost: number;
  avgCostPerRequest: number;
}

/** 终端路径候选 */
export interface PathCandidate {
  path: string;
  exists: boolean;
}
