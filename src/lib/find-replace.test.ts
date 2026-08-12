import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  FIND_MATCH_LIMIT,
  findMatches,
  locateMatchInSegments,
  navigateMatch,
} from './find-replace';

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

describe('审计收口 #12：locateMatchInSegments 跨 text-node 选区定位', () => {
  // 模拟拼接全文 "hello world foo" 由三个 text node 组成：
  // node0 "hello " (0..6), node1 "world " (6..12), node2 "foo" (12..15)
  const segments = [
    { start: 0, length: 6 },
    { start: 6, length: 6 },
    { start: 12, length: 3 },
  ];

  it('匹配落在第一个 text node', () => {
    assert.deepEqual(locateMatchInSegments(0, 5, segments), {
      segmentIndex: 0,
      nodeOffset: 0,
      clampLength: 5,
    });
  });

  it('匹配起点落在第二个 text node（跨节点选区）', () => {
    assert.deepEqual(locateMatchInSegments(7, 11, segments), {
      segmentIndex: 1,
      nodeOffset: 1,
      clampLength: 4,
    });
  });

  it('匹配终点超出所在段时 clamp 到段长（不越界）', () => {
    assert.deepEqual(locateMatchInSegments(10, 14, segments), {
      segmentIndex: 1,
      nodeOffset: 4,
      clampLength: 2, // node1 只剩 2 个字符
    });
  });

  it('空段返回 null', () => {
    assert.equal(locateMatchInSegments(0, 1, []), null);
  });
});
