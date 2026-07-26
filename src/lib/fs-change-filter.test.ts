import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { isNoisyChangePath, topChildOf, SelfOpenedTracker } from './fs-change-filter';

describe('isNoisyChangePath', () => {
  it('放行普通文件变更', () => {
    assert.equal(isNoisyChangePath('/Users/a/proj/src/main.ts', '/Users/a/proj'), false);
    assert.equal(isNoisyChangePath('/Users/a/proj/README.md', '/Users/a/proj'), false);
  });

  it('拒绝相对根目录内以点开头的路径段', () => {
    assert.equal(isNoisyChangePath('/Users/a/proj/.git/index', '/Users/a/proj'), true);
    assert.equal(isNoisyChangePath('/Users/a/proj/.DS_Store', '/Users/a/proj'), true);
    assert.equal(isNoisyChangePath('/Users/a/proj/src/.cache/x', '/Users/a/proj'), true);
  });

  it('根目录自身含点段时不误杀内部普通变更', () => {
    assert.equal(isNoisyChangePath('/Users/a/.config/foo/bar.toml', '/Users/a/.config/foo'), false);
    // 但内部再出现点段仍拒绝
    assert.equal(isNoisyChangePath('/Users/a/.config/foo/.git/HEAD', '/Users/a/.config/foo'), true);
  });

  it('拒绝构建/依赖目录段', () => {
    assert.equal(isNoisyChangePath('/p/node_modules/x/index.js', '/p'), true);
    assert.equal(isNoisyChangePath('/p/target/debug/app', '/p'), true);
    assert.equal(isNoisyChangePath('/p/dist/bundle.js', '/p'), true);
  });

  it('拒绝临时/锁/数据库伴生文件后缀', () => {
    assert.equal(isNoisyChangePath('/p/a.swp', '/p'), true);
    assert.equal(isNoisyChangePath('/p/a.txt~', '/p'), true);
    assert.equal(isNoisyChangePath('/p/pic.png.part', '/p'), true);
    assert.equal(isNoisyChangePath('/p/dl.crdownload', '/p'), true);
    assert.equal(isNoisyChangePath('/p/pkg.lock', '/p'), true);
    assert.equal(isNoisyChangePath('/p/db.sqlite-journal', '/p'), true);
    assert.equal(isNoisyChangePath('/p/db.sqlite-wal', '/p'), true);
    assert.equal(isNoisyChangePath('/p/db.sqlite-shm', '/p'), true);
  });

  it('拒绝中段 .tmp（fanbox：foo.swift.tmp.<pid>.<hex>）', () => {
    assert.equal(isNoisyChangePath('/p/foo.swift.tmp.123.abc', '/p'), true);
    assert.equal(isNoisyChangePath('/p/note.tmp', '/p'), true);
  });

  it('.tmp 规则不误杀含 tmp 子串的正常名', () => {
    assert.equal(isNoisyChangePath('/p/template.ts', '/p'), false);
    assert.equal(isNoisyChangePath('/p/tmpdir-notes.md', '/p'), false);
  });

  it('根目录路径自身不算噪声', () => {
    assert.equal(isNoisyChangePath('/Users/a/proj', '/Users/a/proj'), false);
  });
});

describe('topChildOf', () => {
  it('直接子文件返回自身', () => {
    assert.equal(topChildOf('/root', '/root/a.txt'), '/root/a.txt');
  });

  it('深层路径归并到顶层子目录', () => {
    assert.equal(topChildOf('/root', '/root/src/deep/b.ts'), '/root/src');
  });

  it('不在根目录下返回 null', () => {
    assert.equal(topChildOf('/root', '/other/a.txt'), null);
    assert.equal(topChildOf('/root', '/rootx/a.txt'), null);
  });

  it('根目录带尾斜杠时行为一致', () => {
    assert.equal(topChildOf('/root/', '/root/src/x'), '/root/src');
  });
});

describe('SelfOpenedTracker', () => {
  it('窗口期内视为自噪声，过期后放行并清理', () => {
    const tr = new SelfOpenedTracker(3000);
    tr.mark('/p/a.md', 1000);
    assert.equal(tr.isSelfNoise('/p/a.md', 2000), true);
    assert.equal(tr.isSelfNoise('/p/a.md', 4001), false);
    // 过期即删：再查仍为 false（不残留）
    assert.equal(tr.isSelfNoise('/p/a.md', 2000), false);
  });

  it('未登记路径不受影响', () => {
    const tr = new SelfOpenedTracker();
    assert.equal(tr.isSelfNoise('/p/b.md'), false);
  });

  it('超出上限时淘汰最旧登记', () => {
    const tr = new SelfOpenedTracker(3000, 2);
    tr.mark('/p/1', 100);
    tr.mark('/p/2', 200);
    tr.mark('/p/3', 300);
    assert.equal(tr.isSelfNoise('/p/1', 400), false); // 已被淘汰
    assert.equal(tr.isSelfNoise('/p/2', 400), true);
    assert.equal(tr.isSelfNoise('/p/3', 400), true);
  });
});
