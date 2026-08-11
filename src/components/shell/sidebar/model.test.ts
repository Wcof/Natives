import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { FILE_MANAGER_DIRS, QUICK_ACCESS_ITEMS } from './model';

describe('FILE_MANAGER_DIRS（问题5：三个固定目录为无箭头导航项）', () => {
  it('恰好包含 desktop/documents/downloads 三个固定目录', () => {
    assert.deepEqual(
      FILE_MANAGER_DIRS.map((item) => item.id),
      ['desktop', 'documents', 'downloads'],
    );
  });

  it('documents 指向真实 ~/Documents（修复误指 ~/.natives）', () => {
    const documents = FILE_MANAGER_DIRS.find((item) => item.id === 'documents');
    assert.ok(documents);
    assert.equal(documents.path, '~/Documents');
    assert.equal(documents.target, '__files__:~/Documents');
  });

  it('三个目录都是固定导航项（非目录树，path 与 target 一一对应）', () => {
    for (const item of FILE_MANAGER_DIRS) {
      assert.ok(item.path, `path for ${item.id}`);
      assert.equal(item.target, `__files__:${item.path}`);
    }
  });
});

describe('QUICK_ACCESS_ITEMS（快速访问只保留主页）', () => {
  it('只含 home，不再混入目录树项', () => {
    assert.deepEqual(
      QUICK_ACCESS_ITEMS.map((item) => item.id),
      ['home'],
    );
    assert.equal(QUICK_ACCESS_ITEMS[0]?.target, '__dashboard__');
    assert.equal(QUICK_ACCESS_ITEMS[0]?.path, undefined);
  });
});
