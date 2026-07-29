import assert from 'node:assert/strict';
import { test } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { MarkdownPreviewProps } from '@uiw/react-markdown-preview';
import { fileMarkdownRenderProps, fileMarkdownUrlTransform } from './FileMarkdownPreview';

function StubPreview(props: MarkdownPreviewProps) {
  const source = props.source ?? '';
  const title = source.match(/^# (.+)$/m)?.[1] ?? '';
  const image = source.match(/!\[[^\]]*\]\(([^)]+)\)/)?.[1] ?? '';
  return (
    <article>
      <h1>{title}</h1>
      {image ? <img src={props.urlTransform?.(image, 'src', { tagName: 'img' } as never) ?? undefined} alt="" /> : null}
    </article>
  );
}

test('file markdown is rendered as markup and keeps rewritten local images', () => {
  const html = renderToStaticMarkup(
    <StubPreview {...fileMarkdownRenderProps('# Title\n\n![cover](asset://localhost/tmp/cover.png)')} />,
  );

  assert.match(html, /<h1>Title<\/h1>/);
  assert.doesNotMatch(html, /# Title/);
  assert.match(html, /asset:\/\/localhost\/tmp\/cover\.png/);
});

test('file markdown allows local asset images but rejects active URLs', () => {
  assert.equal(
    fileMarkdownUrlTransform('asset://localhost/tmp/a.png', 'src', { tagName: 'img' }),
    'asset://localhost/tmp/a.png',
  );
  assert.equal(fileMarkdownUrlTransform('javascript:alert(1)', 'href', { tagName: 'a' }), '');
});
