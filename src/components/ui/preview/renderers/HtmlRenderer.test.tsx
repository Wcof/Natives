// P2-03 · HtmlRenderer 接线测试
//
// HTML lane 垂直链（PREV-001 → P2-01 → P2-03）：provider 冻结 sandbox 红线
// （R-S2：allow-scripts allow-forms；无 allow-same-origin / allow-top-navigation
// / allow-popups），renderer 以 srcDoc 呈现且只透传 model.sandbox，绝不自行放宽。
// 使用 SSR（renderToStaticMarkup）：iframe 属性可直接断言。

import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import HtmlRenderer from './HtmlRenderer';
import { PreviewRenderer } from '../PreviewRenderer';

const SANDBOX = 'allow-scripts allow-forms';

test('HtmlRenderer renders srcDoc with the provider-frozen sandbox', () => {
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
  assert.match(html, /<iframe/);
  // React SSR 把 srcDoc prop 序列化为 camelCase 属性（客户端渲染时直接设 DOM 属性）；
  // 两种形态都接受，只验证 srcDoc 内容真正承载在 iframe 上
  assert.match(html, /srcDoc=|srcdoc=/);
  assert.ok(html.includes('allow-scripts'));
  assert.ok(html.includes('allow-forms'));
  // sandbox 红线：禁止同源提升 / 顶层导航 / popup
  assert.ok(!html.includes('allow-same-origin'));
  assert.ok(!html.includes('allow-top-navigation'));
  assert.ok(!html.includes('allow-popups'));
  // Host 已把本地引用改写为 /fs/{token}/；srcDoc 直接承载
  assert.ok(html.includes('http://localhost:4321/fs/tok/a.html'));
  assert.ok(html.includes('data-preview-kind="html"'));
});

test('HtmlRenderer never widens the sandbox beyond the model value', () => {
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
  // sandbox 属性必须等于 model.sandbox（无额外 token）
  assert.ok(html.includes(`sandbox="${SANDBOX}"`));
  assert.ok(!html.includes('allow-same-origin'));
  assert.ok(!html.includes('allow-top-navigation'));
  assert.ok(!html.includes('allow-popups'));
});

test('PreviewRenderer dispatches html model to HtmlRenderer (no typeNotEnabled placeholder)', () => {
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
  assert.match(html, /<iframe/);
  assert.match(html, /srcDoc=|srcdoc=/);
  // 旧占位语义（typeNotEnabled / html-unsupported）必须不可达
  assert.ok(!html.includes('typeNotEnabled'));
  assert.ok(!html.includes('html-unsupported'));
});

