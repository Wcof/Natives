import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { resolveTargetPaths } from './useFileOperations';

/**
 * F3-04 · resolveTargetPaths 特性测试（ARCH-002 拆分后的纯函数）。
 * 目标路径解析优先级：多选集合 > 传入条目 > 光标条目（selectedIndex）> 空。
 * 与原 FileBrowser resolveTargetPaths 语义完全一致。
 */

interface Entry {
  path: string;
  isDir: boolean;
}

const ENTRIES: Entry[] = [
  { path: '/a', isDir: true },
  { path: '/b', isDir: false },
  { path: '/c', isDir: false },
];

describe('resolveTargetPaths', () => {
  it('prefers multi-selection over entry and cursor', () => {
    const sel = new Set(['/b', '/c']);
    const paths = resolveTargetPaths(
      sel,
      0,
      ENTRIES as never[],
      { path: '/a', isDir: true } as never,
    );
    assert.deepEqual(paths, ['/b', '/c']);
  });

  it('falls back to the provided entry when selection is empty', () => {
    const paths = resolveTargetPaths(new Set(), -1, ENTRIES as never[], { path: '/c', isDir: false } as never);
    assert.deepEqual(paths, ['/c']);
  });

  it('falls back to cursor entry when neither selection nor entry', () => {
    const paths = resolveTargetPaths(new Set(), 1, ENTRIES as never[], undefined);
    assert.deepEqual(paths, ['/b']);
  });

  it('returns empty when nothing is resolvable', () => {
    assert.deepEqual(resolveTargetPaths(new Set(), -1, ENTRIES as never[], undefined), []);
    assert.deepEqual(resolveTargetPaths(new Set(), 99, ENTRIES as never[], undefined), []);
  });

  it('single-path selection wins over provided entry', () => {
    const paths = resolveTargetPaths(
      new Set(['/b']),
      2,
      ENTRIES as never[],
      { path: '/c', isDir: false } as never,
    );
    assert.deepEqual(paths, ['/b']);
  });
});
