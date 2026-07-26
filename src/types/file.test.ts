import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type {
  FileEntry,
  FileKind,
  ProjectBadge,
  StatResult,
  SearchResult,
  ContentSearchResult,
  GitFileStatus,
  DiskUsageItem,
  PathCandidate,
} from './file';

// detectFileKind / detectProjectBadge 的行为测试已随实现移除——
// kind 与项目徽章由 Rust 单一来源计算并随 wire 下发
// （src-tauri/src/file_manager.rs 的 detect_file_kind / detect_project_badge，
// Rust 侧单测覆盖），前端不再有第二份实现可测。
// 本文件保留 wire 类型的编译期形状断言：字段名/类型漂移会在 tsc 阶段报错。

describe('FileTypes', () => {
  it('FileEntry wire shape stays camelCase and complete', () => {
    const entry: FileEntry = {
      name: 'a.md',
      path: '/p/a.md',
      isDir: false,
      kind: 'text',
      hidden: false,
      size: 1,
      mtime: 2,
      btime: 3,
      dirHint: '/p',
    };
    assert.equal(entry.kind, 'text');

    // kind 联合类型必须含目录 "dir"（生成自 Rust）
    const dirKind: FileKind = 'dir';
    assert.equal(dirKind, 'dir');

    const badge: ProjectBadge = 'node';
    assert.equal(badge, 'node');
  });

  it('StatResult allows missing-file shape', () => {
    const missing: StatResult = { found: false, path: '/nope' };
    assert.equal(missing.found, false);
  });

  it('frontend view models keep their normalized shapes', () => {
    const s: SearchResult = { path: '/p', name: 'p', score: 1, isDir: false, mtime: 0, matchRanges: [[0, 1]] };
    const c: ContentSearchResult = { path: '/p', name: 'p', line: 1, preview: 'x', matchStart: 0, matchEnd: 1 };
    const g: GitFileStatus = { path: '/p', status: 'M' };
    const d: DiskUsageItem = { name: 'n', path: '/p', isDir: true, size: 0, sizeFormatted: '0 B' };
    const pc: PathCandidate = { path: '/p', exists: true };
    assert.ok(s && c && g && d && pc);
  });
});
