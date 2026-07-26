/**
 * Pure safety helpers for assistant Markdown (no CSS / React runtime).
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  isSafeMarkdownUrl,
  isSafeImageSource,
  transformMarkdownUrl,
  pluginsFilter,
  rewriteMarkdownNode,
  SAFE_MARKDOWN_ELEMENTS,
} from './markdown-safety';

test('allows http(s), mailto, anchors, and relative paths', () => {
  assert.equal(isSafeMarkdownUrl('https://example.com/a'), true);
  assert.equal(isSafeMarkdownUrl('http://example.com'), true);
  assert.equal(isSafeMarkdownUrl('mailto:a@b.com'), true);
  assert.equal(isSafeMarkdownUrl('#section'), true);
  assert.equal(isSafeMarkdownUrl('./docs/readme.md'), true);
  assert.equal(isSafeMarkdownUrl('/local/path'), true);
  assert.equal(isSafeMarkdownUrl('docs/readme.md'), true);
});

test('rejects javascript/data/file/vbscript and protocol-relative URLs', () => {
  assert.equal(isSafeMarkdownUrl('javascript:alert(1)'), false);
  assert.equal(isSafeMarkdownUrl('JAVASCRIPT:alert(1)'), false);
  assert.equal(isSafeMarkdownUrl('data:text/html;base64,abc'), false);
  assert.equal(isSafeMarkdownUrl('file:///etc/passwd'), false);
  assert.equal(isSafeMarkdownUrl('//evil.example/x'), false);
  assert.equal(isSafeMarkdownUrl('vbscript:msgbox(1)'), false);
  assert.equal(transformMarkdownUrl('javascript:alert(1)'), '');
  assert.equal(transformMarkdownUrl('https://ok.example'), 'https://ok.example');
  assert.equal(transformMarkdownUrl('#ok'), '#ok');
});

test('pluginsFilter strips rehype-raw from rehype list only', () => {
  const rawPlugin = function rehypeRaw() {
    return () => undefined;
  };
  const other = function rehypeSlug() {
    return () => undefined;
  };
  const filtered = pluginsFilter('rehype', [rawPlugin, other, [rawPlugin, {}]]) as unknown[];
  assert.equal(filtered.length, 1);
  assert.equal(filtered[0], other);
  assert.deepEqual(pluginsFilter('remark', [rawPlugin]), [rawPlugin]);
});

test('rewriteMarkdownNode neuters script and event/style attributes', () => {
  const script = {
    type: 'element',
    tagName: 'script',
    properties: {},
    children: [{ type: 'text', value: 'alert(1)' }],
  };
  rewriteMarkdownNode(script);
  assert.equal(script.type, 'text');

  const anchor = {
    type: 'element',
    tagName: 'a',
    properties: {
      href: 'javascript:alert(1)',
      onclick: 'evil()',
      style: 'color:red',
    },
  };
  rewriteMarkdownNode(anchor);
  assert.equal(anchor.properties.href, '');
  assert.equal('onclick' in anchor.properties, false);
  assert.equal('style' in anchor.properties, false);

  const safe = {
    type: 'element',
    tagName: 'a',
    properties: { href: 'https://example.com' } as Record<string, unknown>,
  };
  rewriteMarkdownNode(safe);
  assert.equal(safe.properties.href, 'https://example.com');
  assert.equal(safe.properties.target, '_blank');
  assert.equal(safe.properties.rel, 'noopener noreferrer nofollow');
});

test('SAFE_MARKDOWN_ELEMENTS includes GFM structure and excludes script/style', () => {
  assert.ok(SAFE_MARKDOWN_ELEMENTS.includes('table'));
  assert.ok(SAFE_MARKDOWN_ELEMENTS.includes('code'));
  assert.ok(SAFE_MARKDOWN_ELEMENTS.includes('input'));
  assert.ok(!(SAFE_MARKDOWN_ELEMENTS as readonly string[]).includes('script'));
  assert.ok(!(SAFE_MARKDOWN_ELEMENTS as readonly string[]).includes('style'));
  assert.ok(!(SAFE_MARKDOWN_ELEMENTS as readonly string[]).includes('iframe'));
});

test('img src allows base64 raster data URLs only (G13)', () => {
  const png = 'data:image/png;base64,iVBORw0KGgo=';
  const webp = 'data:image/webp;base64,AAAA';
  const jpeg = 'data:image/jpeg;base64,AAAA';
  assert.equal(isSafeImageSource(png), true);
  assert.equal(isSafeImageSource(webp), true);
  assert.equal(isSafeImageSource(jpeg), true);
  assert.equal(isSafeImageSource('data:image/svg+xml;base64,AAAA'), false, 'svg can script');
  assert.equal(isSafeImageSource('data:text/html;base64,AAAA'), false);
  assert.equal(isSafeImageSource('data:image/png,notbase64'), false);
  assert.equal(isSafeImageSource('https://example.com/x.png'), true);

  // urlTransform is tag-aware: img src passes, everything else keeps rejecting data:
  assert.equal(transformMarkdownUrl(png, 'src', { tagName: 'img' }), png);
  assert.equal(transformMarkdownUrl(png, 'href', { tagName: 'a' }), '');
  assert.equal(transformMarkdownUrl(png), '');
  assert.equal(
    transformMarkdownUrl('data:image/svg+xml;base64,AAAA', 'src', { tagName: 'img' }),
    '',
  );

  // rehype rewrite mirrors the same rule
  const img = {
    type: 'element',
    tagName: 'img',
    properties: { src: png } as Record<string, unknown>,
  };
  rewriteMarkdownNode(img);
  assert.equal(img.properties.src, png);
  const evilImg = {
    type: 'element',
    tagName: 'img',
    properties: { src: 'data:text/html;base64,AAAA' } as Record<string, unknown>,
  };
  rewriteMarkdownNode(evilImg);
  assert.equal(evilImg.properties.src, '');
});

test('table gets scroll-container class without widening the allowlist (G8)', () => {
  const table = {
    type: 'element',
    tagName: 'table',
    properties: {} as Record<string, unknown>,
    children: [],
  };
  rewriteMarkdownNode(table);
  assert.deepEqual(table.properties.className, ['md-table-overflow']);

  // idempotent + preserves existing classes
  const classed = {
    type: 'element',
    tagName: 'table',
    properties: { className: 'foo md-table-overflow' } as Record<string, unknown>,
  };
  rewriteMarkdownNode(classed);
  assert.deepEqual(classed.properties.className, ['foo', 'md-table-overflow']);

  // div stays out of SAFE_MARKDOWN_ELEMENTS (no wrapper element security widening)
  assert.ok(!(SAFE_MARKDOWN_ELEMENTS as readonly string[]).includes('div'));
});

test('empty / whitespace sources are considered non-content by callers', () => {
  assert.equal('   '.trim().length, 0);
  assert.equal(''.trim().length, 0);
  assert.ok('# hi'.trim().length > 0);
});
