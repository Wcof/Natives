// ── 自动生成类型的统一出口 ──
//
// 本目录下除本文件外的所有 *.ts 均由 ts-rs 从 src-tauri 的 Rust struct 生成，
// 是文件管理器域前后端契约的单一来源（Single Source of Truth）。
// 请勿手改生成文件；修改契约请改 Rust struct 后运行：
//
//   npm run types:generate
//
// 来源映射（Rust struct → 生成文件）：
//   src-tauri/src/file_manager.rs → FileEntry / ListDirOptions / ListDirResult
//                                   / ReadFileResult / WriteResult / StatResult
//   src-tauri/src/fs_watch.rs     → FsWatchEvent
//   src-tauri/src/search.rs       → SearchResult
//   src-tauri/src/archive.rs      → ArchiveEntry / ArchiveListing
//   src-tauri/src/disk_usage.rs   → DiskUsageItem

export type { FileEntry } from './FileEntry';
export type { ListDirOptions } from './ListDirOptions';
export type { ListDirResult } from './ListDirResult';
export type { ReadFileResult } from './ReadFileResult';
export type { WriteResult } from './WriteResult';
export type { StatResult } from './StatResult';
export type { FsWatchEvent } from './FsWatchEvent';
export type { SearchResult } from './SearchResult';
export type { ArchiveEntry } from './ArchiveEntry';
export type { ArchiveListing } from './ArchiveListing';
export type { DiskUsageItem } from './DiskUsageItem';
