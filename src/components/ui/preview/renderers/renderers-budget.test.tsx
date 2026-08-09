// P2-02 · Renderer 展示级预算测试
//
// 预算链：provider（read/parse 字节/行/列/节点）→ renderer（DOM 行/列/条目上限）。
// 本文件验证 renderer 不把超限内容一次性塞进 DOM（R-P4）并显式标记 truncated。
// 使用 SSR（renderToStaticMarkup）：useEffect 不执行，CodeRenderer 落回有界纯文本
// pre（正好可断言 DOM 上限）。

import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// tsx --test uses the classic JSX transform; mirror what Next injects at build.
(globalThis as { React?: typeof React }).React = React;

import ArchiveRenderer, { ARCHIVE_RENDER_MAX_ENTRIES } from './ArchiveRenderer';
import CodeRenderer, { CODE_RENDER_MAX_LINES } from './CodeRenderer';
import CsvRenderer, { CSV_RENDER_MAX_ROWS, CSV_RENDER_MAX_COLUMNS } from './CsvRenderer';
import JsonRenderer from './JsonRenderer';

test('CsvRenderer caps rows in DOM and shows more-rows note', () => {
  const rows = Array.from({ length: CSV_RENDER_MAX_ROWS + 10 }, (_, i) => [`r${i}`]);
  const html = renderToStaticMarkup(
    React.createElement(CsvRenderer, {
      model: { kind: 'csv', headers: ['c'], rows, truncated: false },
    }),
  );
  assert.ok(html.includes('未显示'), 'should show more-rows note');
  assert.ok(!html.includes(`>r${CSV_RENDER_MAX_ROWS + 5}<`), 'capped rows must not hit DOM');
});

test('CsvRenderer caps columns in DOM and shows more-columns note', () => {
  const headers = Array.from({ length: CSV_RENDER_MAX_COLUMNS + 5 }, (_, i) => `h${i}`);
  const html = renderToStaticMarkup(
    React.createElement(CsvRenderer, {
      model: { kind: 'csv', headers, rows: [['x']], truncated: false },
    }),
  );
  assert.ok(html.includes('仅显示前'), 'should show more-columns note');
  assert.ok(!html.includes(`>h${CSV_RENDER_MAX_COLUMNS + 2}<`), 'capped columns must not hit DOM');
});

test('CodeRenderer bounds DOM lines and flags truncation', () => {
  const lines = Array.from({ length: CODE_RENDER_MAX_LINES + 50 }, (_, i) => `line${i}`);
  const html = renderToStaticMarkup(
    React.createElement(CodeRenderer, {
      model: { kind: 'code', source: lines.join('\n'), language: 'text', truncated: false },
    }),
  );
  assert.ok(html.includes('已截断显示前'), 'should show truncation note');
  assert.ok(!html.includes(`line${CODE_RENDER_MAX_LINES + 5}`), 'capped lines must not hit DOM');
});

test('ArchiveRenderer caps entries in DOM and flags truncation', () => {
  const entries = Array.from({ length: ARCHIVE_RENDER_MAX_ENTRIES + 20 }, (_, i) => ({
    name: `f${i}.txt`,
    size: i,
    isDir: false,
  }));
  const html = renderToStaticMarkup(
    React.createElement(ArchiveRenderer, {
      model: { kind: 'archive', entries, truncated: true },
    }),
  );
  assert.ok(html.includes('压缩包条目较多'), 'should show archive truncated note');
  assert.ok(!html.includes(`f${ARCHIVE_RENDER_MAX_ENTRIES + 10}.txt`), 'capped entries must not hit DOM');
});

test('JsonRenderer renders bounded value with truncated warning', () => {
  const html = renderToStaticMarkup(
    React.createElement(JsonRenderer, {
      model: { kind: 'json', value: { a: 1, list: [1, 2, 3] }, formatted: '', nodeCount: 400, truncated: true },
    }),
  );
  assert.ok(html.includes('JSON 超出'), 'should show json budget warning');
  assert.ok(html.includes('a'), 'should still render bounded value');
});
