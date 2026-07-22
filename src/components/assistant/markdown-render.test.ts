/**
 * Structure-level Markdown rendering tests for assistant text/plan blocks.
 * Uses a stub Preview + lightweight host that mirrors MarkdownTextView props.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import React, { type ComponentType, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { MarkdownPreviewProps } from '@uiw/react-markdown-preview';
import {
  transformMarkdownUrl,
  rewriteMarkdownNode,
  SAFE_MARKDOWN_ELEMENTS,
  pluginsFilter,
} from './markdown-safety';

function StubPreview(props: MarkdownPreviewProps) {
  const source = props.source ?? '';
  assert.ok(Array.isArray(props.allowedElements));
  assert.deepEqual(props.allowedElements, [...SAFE_MARKDOWN_ELEMENTS]);
  assert.equal(props.skipHtml, true);
  assert.equal(typeof props.urlTransform, 'function');

  const lines = source.split('\n');
  const nodes: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? '';
    if (line.startsWith('```')) {
      const lang = line.slice(3).trim();
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !(lines[i] ?? '').startsWith('```')) {
        body.push(lines[i] ?? '');
        i += 1;
      }
      nodes.push(
        React.createElement(
          'pre',
          { key: `c-${i}`, 'data-lang': lang || undefined },
          React.createElement('code', null, body.join('\n')),
        ),
      );
      if ((lines[i] ?? '').startsWith('```')) i += 1;
      continue;
    }
    if (line.startsWith('# ')) {
      nodes.push(React.createElement('h1', { key: `h-${i}` }, line.slice(2)));
      i += 1;
      continue;
    }
    if (line.startsWith('|') && line.includes('|', 1)) {
      const rows: string[][] = [];
      while (i < lines.length && (lines[i] ?? '').startsWith('|')) {
        const raw = lines[i] ?? '';
        if (!/^\|\s*-+/.test(raw)) {
          rows.push(
            raw
              .split('|')
              .slice(1, -1)
              .map((c) => c.trim()),
          );
        }
        i += 1;
      }
      nodes.push(
        React.createElement(
          'div',
          { key: `t-${i}`, 'data-markdown-table': true },
          React.createElement(
            'table',
            null,
            React.createElement(
              'tbody',
              null,
              rows.map((r, ri) =>
                React.createElement(
                  'tr',
                  { key: ri },
                  r.map((c, ci) => React.createElement('td', { key: ci }, c)),
                ),
              ),
            ),
          ),
        ),
      );
      continue;
    }
    if (/^- \[[ xX]\] /.test(line)) {
      const checked = /^- \[[xX]\] /.test(line);
      nodes.push(
        React.createElement(
          'ul',
          { key: `task-${i}`, className: 'contains-task-list' },
          React.createElement(
            'li',
            { className: 'task-list-item' },
            React.createElement('input', {
              type: 'checkbox',
              checked,
              disabled: true,
              readOnly: true,
            }),
            React.createElement('span', null, line.replace(/^- \[[ xX]\] /, '')),
          ),
        ),
      );
      i += 1;
      continue;
    }
    if (/^[-*] /.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^[-*] /.test(lines[i] ?? '')) {
        items.push((lines[i] ?? '').replace(/^[-*] /, ''));
        i += 1;
      }
      nodes.push(
        React.createElement(
          'ul',
          { key: `ul-${i}` },
          items.map((item, idx) => React.createElement('li', { key: idx }, item)),
        ),
      );
      continue;
    }
    if (/^\d+\. /.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\d+\. /.test(lines[i] ?? '')) {
        items.push((lines[i] ?? '').replace(/^\d+\. /, ''));
        i += 1;
      }
      if (i < lines.length && /^\s{2,}[-*] /.test(lines[i] ?? '')) {
        const nested: string[] = [];
        while (i < lines.length && /^\s{2,}[-*] /.test(lines[i] ?? '')) {
          nested.push((lines[i] ?? '').replace(/^\s{2,}[-*] /, ''));
          i += 1;
        }
        nodes.push(
          React.createElement(
            'ol',
            { key: `ol-${i}` },
            items.map((item, idx) =>
              React.createElement(
                'li',
                { key: idx },
                item,
                idx === items.length - 1 && nested.length > 0
                  ? React.createElement(
                      'ul',
                      null,
                      nested.map((n, ni) => React.createElement('li', { key: ni }, n)),
                    )
                  : null,
              ),
            ),
          ),
        );
        continue;
      }
      nodes.push(
        React.createElement(
          'ol',
          { key: `ol-${i}` },
          items.map((item, idx) => React.createElement('li', { key: idx }, item)),
        ),
      );
      continue;
    }
    if (line.startsWith('[') && line.includes('](')) {
      const m = /\[([^\]]+)\]\(([^)]+)\)/.exec(line);
      if (m) {
        const href = transformMarkdownUrl(m[2]!);
        nodes.push(
          React.createElement(
            'p',
            { key: `a-${i}` },
            href
              ? React.createElement('a', { href }, m[1])
              : m[1],
          ),
        );
        i += 1;
        continue;
      }
    }
    if (line.trim()) {
      nodes.push(React.createElement('p', { key: `p-${i}` }, line));
    }
    i += 1;
  }

  return React.createElement('div', { 'data-testid': 'stub-preview' }, nodes);
}

/** Mirrors MarkdownTextView prop contract without CSS modules. */
function renderWithContract(source: string, Preview: ComponentType<MarkdownPreviewProps>): string {
  return renderToStaticMarkup(
    React.createElement(
      'div',
      { 'data-assistant-markdown': true },
      React.createElement(Preview, {
        source,
        skipHtml: true,
        disableCopy: true,
        urlTransform: transformMarkdownUrl,
        allowedElements: [...SAFE_MARKDOWN_ELEMENTS],
        unwrapDisallowed: true,
        pluginsFilter: pluginsFilter as MarkdownPreviewProps['pluginsFilter'],
        rehypeRewrite: rewriteMarkdownNode as MarkdownPreviewProps['rehypeRewrite'],
      }),
    ),
  );
}

function render(source: string): string {
  return renderWithContract(source, StubPreview);
}

test('renders headings, nested lists, tables, task lists, and fenced code', () => {
  const html = render(`# Title

1. One
   - nested
2. Two

| A | B |
| - | - |
| 1 | 2 |

- [x] done
- [ ] todo

\`\`\`ts
const x = 1;
\`\`\`
`);
  assert.match(html, /<h1[^>]*>Title<\/h1>/);
  assert.match(html, /<ol>/);
  assert.match(html, /nested/);
  assert.match(html, /<table>/);
  assert.match(html, /task-list-item/);
  assert.match(html, /type="checkbox"/);
  assert.match(html, /const x = 1;/);
  assert.match(html, /data-lang="ts"/);
  assert.doesNotMatch(html, /# Title/);
  assert.doesNotMatch(html, /```ts/);
});

test('unclosed code fence does not throw', () => {
  assert.doesNotThrow(() => {
    const html = render('```js\nconsole.log(1);');
    assert.match(html, /console\.log\(1\);/);
  });
});

test('raw HTML script nodes are rewritten away', () => {
  const node = {
    type: 'element',
    tagName: 'script',
    properties: {},
    children: [{ type: 'text', value: 'alert(1)' }],
  };
  rewriteMarkdownNode(node);
  assert.notEqual((node as { tagName?: string }).tagName, 'script');
  assert.equal(node.type, 'text');
});

test('https links and page anchors preserved; javascript/data rejected', () => {
  const good = render('[docs](https://example.com/a)\n\n[jump](#section)');
  assert.match(good, /href="https:\/\/example\.com\/a"/);
  assert.match(good, /href="#section"/);

  const bad = render('[x](javascript:alert(1))\n\n[y](data:text/html,hi)');
  assert.doesNotMatch(bad, /href="javascript:/);
  assert.doesNotMatch(bad, /href="data:/);
});

test('markdown host mounts with data-assistant-markdown marker', () => {
  const html = render('hello **world**');
  assert.match(html, /data-assistant-markdown/);
  assert.match(html, /hello/);
});
