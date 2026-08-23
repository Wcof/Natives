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
//   src-tauri/src/archive_ops.rs  → ExtractArchiveResult / CompressEntriesResult
//   src-tauri/src/image_convert.rs → ConvertImageResult
//   src-tauri/src/locate.rs       → VerifyPathResult / LocateResult
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
export type { ExtractArchiveResult } from './ExtractArchiveResult';
export type { CompressEntriesResult } from './CompressEntriesResult';
export type { ConvertImageResult } from './ConvertImageResult';
export type { VerifyPathResult } from './VerifyPathResult';
export type { LocateResult } from './LocateResult';
export type { DiskUsageItem } from './DiskUsageItem';
export type { App } from './App';
export type { AppKind } from './AppKind';
export type { RegistrationOrigin } from './RegistrationOrigin';
export type { AppRuntimeState } from './AppRuntimeState';
export type { AppCapabilities } from './AppCapabilities';
export type { AppView } from './AppView';
export type { RuntimeInstance } from './RuntimeInstance';
