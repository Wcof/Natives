// H0 remains BLOCKED: even a stale/injected html model must fail closed.

import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import HtmlRenderer from './HtmlRenderer';
import { PreviewRenderer } from '../PreviewRenderer';

const SANDBOX = 'allow-scripts allow-forms';

test('HtmlRenderer never mounts executable HTML from a supplied html model', () => {
  const html = renderToStaticMarkup(
    React.createElement(HtmlRenderer, {
      model: {
        kind: 'html',
        revision: '/docs/a.html:1:1',
        html: '<img src="http://localhost:4321/fs/tok/a.html">',
        sandbox: SANDBOX,
      },
    }),
  );
  assert.ok(!html.includes('<iframe'));
  assert.ok(!html.includes('srcDoc='));
  assert.ok(!html.includes('srcdoc='));
  assert.ok(!html.includes('http://localhost:4321/fs/tok/a.html'));
  assert.ok(html.includes('data-preview-kind="html-blocked"'));
});

test('HtmlRenderer discards injected scripts instead of rendering them as source', () => {
  const html = renderToStaticMarkup(
    React.createElement(HtmlRenderer, {
      model: {
        kind: 'html',
        revision: 'memory:a.html:4',
        html: '<script>window.parent.postMessage("*","*")</script>',
        sandbox: SANDBOX,
      },
    }),
  );
  assert.ok(!html.includes('<iframe'));
  assert.ok(!html.includes('postMessage'));
  assert.ok(html.includes('role="status"'));
});

test('PreviewRenderer dispatches a defensive html model to the fail-closed renderer', () => {
  const html = renderToStaticMarkup(
    React.createElement(PreviewRenderer, {
      model: {
        kind: 'html',
        revision: '/docs/a.html:1:1',
        html: '<h1>hi</h1>',
        sandbox: SANDBOX,
      },
    }),
  );
  assert.ok(!html.includes('<iframe'));
  assert.ok(!html.includes('<h1>hi</h1>'));
  assert.ok(html.includes('data-preview-kind="html-blocked"'));
});
