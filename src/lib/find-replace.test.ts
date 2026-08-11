import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { FIND_MATCH_LIMIT, findMatches, navigateMatch } from './find-replace';

describe('findMatches', () => {
  it('空查询 / 空文本返回空', () => {
    assert.deepEqual(findMatches('abc', '  '), []);
    assert.deepEqual(findMatches('', 'a'), []);
  });

  it('默认大小写不敏感', () => {
    assert.deepEqual(findMatches('Foo foo FOO', 'foo'), [
      { start: 0, end: 3 },
      { start: 4, end: 7 },
      { start: 8, end: 11 },
    ]);
  });

  it('大小写敏感', () => {
    assert.deepEqual(findMatches('Foo foo', 'Foo', { caseSensitive: true }), [
      { start: 0, end: 3 },
    ]);
  });

  it('Unicode 文本按 code unit 定位（与 DOM 一致）', () => {
    // "你好 world 你好" — 每个汉字 2 个 UTF-16 code unit。
    // 0-1=你, 1-2=好, 2=空格, 3-7=world, 8=空格, 9-10=你, 10-11=好
    const text = '你好 world 你好';
    assert.deepEqual(findMatches(text, '你好'), [
      { start: 0, end: 2 },
      { start: 9, end: 11 },
    ]);
  });

  it('有界匹配：超过上限截断', () => {
    const matches = findMatches('a'.repeat(FIND_MATCH_LIMIT + 10), 'a');
    assert.equal(matches.length, FIND_MATCH_LIMIT);
  });
});

describe('navigateMatch', () => {
  it('无匹配返回 -1', () => {
    assert.equal(navigateMatch(0, 0, 'next'), -1);
  });

  it('循环导航', () => {
    assert.equal(navigateMatch(0, 3, 'next'), 1);
    assert.equal(navigateMatch(2, 3, 'next'), 0);
    assert.equal(navigateMatch(0, 3, 'prev'), 2);
  });

  it('当前未选时默认跳首项', () => {
    assert.equal(navigateMatch(-1, 3, 'next'), 0);
    assert.equal(navigateMatch(-1, 3, 'prev'), 2);
  });
});
