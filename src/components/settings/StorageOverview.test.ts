// 问题7：StorageOverview 纯逻辑（真实根卷存储信息）。
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import {
  StorageOverviewContent,
  formatBytes,
  isStorageInfo,
  storagePercent,
  storageUsedLabel,
} from './StorageOverview';

// tsx --test 使用 classic JSX transform；Next 构建时同样注入。
(globalThis as { React?: typeof React }).React = React;

const zh = 'zh' as const;

/** 只读内容投影渲染（loading/error/ready/unavailable 三态）。 */
function renderContent(state: {
  loading: boolean;
  error: string | null;
  info: { totalBytes: number; usedBytes: number; availableBytes: number } | null;
}): string {
  return renderToStaticMarkup(
    React.createElement(StorageOverviewContent, {
      locale: zh,
      loading: state.loading,
      error: state.error,
      info: state.info,
      onRetry: () => undefined,
    }),
  );
}

describe('isStorageInfo', () => {
  it('接受完整非负数值对象', () => {
    assert.equal(isStorageInfo({ totalBytes: 100, usedBytes: 40, availableBytes: 60 }), true);
  });

  it('拒绝缺失字段 / 负数 / 非数值', () => {
    assert.equal(isStorageInfo(null), false);
    assert.equal(isStorageInfo({ totalBytes: 100, usedBytes: 40 }), false);
    assert.equal(isStorageInfo({ totalBytes: -1, usedBytes: 40, availableBytes: 60 }), false);
    assert.equal(isStorageInfo({ totalBytes: 'x', usedBytes: 40, availableBytes: 60 }), false);
  });
});

describe('storagePercent', () => {
  it('used/total 百分比', () => {
    assert.equal(storagePercent({ totalBytes: 200, usedBytes: 50, availableBytes: 150 }), 25);
  });

  it('total 为 0 时返回 null（不伪造 0%）', () => {
    assert.equal(storagePercent({ totalBytes: 0, usedBytes: 0, availableBytes: 0 }), null);
  });

  it('used 超过 total 时封顶 100', () => {
    assert.equal(storagePercent({ totalBytes: 100, usedBytes: 200, availableBytes: 0 }), 100);
  });
});

describe('formatBytes', () => {
  it('进制与单位', () => {
    assert.equal(formatBytes(0), '0 B');
    assert.equal(formatBytes(1024), '1 KB');
    assert.equal(formatBytes(1024 * 1024 * 1536), '1.5 GB');
  });
});

describe('审计收口 #7：百分比文案恰好一个 %', () => {
  it('storageUsedLabel 不含 % 后缀（模板自带 {percent}%），不产生 50%%', () => {
    // zh 模板：'已使用 {percent}%'——传 50 应得 '已使用 50%'，绝无 50%%。
    const label = storageUsedLabel(zh, 50);
    assert.match(label, /50%/);
    assert.equal((label.match(/%/g) ?? []).length, 1, `恰好一个 %，实际: ${label}`);
  });

  it('传 33.7 四舍五入为 34%', () => {
    const label = storageUsedLabel(zh, 33.7);
    assert.match(label, /34%/);
  });
});

describe('审计收口 #7：StorageOverviewContent 三态 render', () => {
  it('loading 渲染加载文案，不出现百分比', () => {
    const html = renderContent({ loading: true, error: null, info: null });
    assert.match(html, /加载/);
    assert.ok(!html.includes('%'), 'loading 不渲染百分比');
  });

  it('error 渲染 unavailable + retry，不出现 0%', () => {
    const html = renderContent({ loading: false, error: '磁盘读失败', info: null });
    assert.match(html, /无法读取/);
    assert.match(html, /重试/);
    assert.ok(!html.includes('%'), 'error 不伪装 0%');
  });

  it('ready 渲染恰好一个 %（progressbar aria-valuenow + 文案 % 各一份）', () => {
    const html = renderContent({
      loading: false,
      error: null,
      info: { totalBytes: 200, usedBytes: 50, availableBytes: 150 },
    });
    assert.match(html, /25%/);
    assert.match(html, /progressbar/);
    assert.match(html, /aria-valuenow="25"/);
    // 文案恰好一个 %（模板自带），progressbar width 是第二个 %——合计 2 个。
    assert.equal((html.match(/%/g) ?? []).length, 2, `文案 1 + width 1，实际: ${html}`);
    assert.ok(!html.includes('%%'), '绝不出现 50%%');
  });

  it('unknown（totalBytes<=0）渲染 unavailable + retry，不渲染 progressbar', () => {
    const html = renderContent({
      loading: false,
      error: null,
      info: { totalBytes: 0, usedBytes: 0, availableBytes: 0 },
    });
    assert.match(html, /无法读取|未知/);
    assert.match(html, /重试/);
    assert.ok(!html.includes('progressbar'), 'unknown 不伪装 0%');
    assert.ok(!html.includes('%'), 'unknown 不出现百分比');
  });
});
