'use client';

/**
 * T19 · PreviewRenderer — 穷尽式 renderer composition（唯一组装者）。
 * 只消费 PreviewModel；不决定「这个 path 该用谁」（那属于 registry/service）。
 * loading/error/fallback 由 PreviewSurface 负责，本组件只负责「模型 → 渲染」。
 * 新 kind 必须在此穷尽分支；H0=BLOCKED 时 html 模型暂由 unsupported 呈现。
 */

import type { PreviewModel } from '@/lib/preview/contracts';
import MarkdownRenderer from './renderers/MarkdownRenderer';
import JsonRenderer from './renderers/JsonRenderer';
import CodeRenderer from './renderers/CodeRenderer';
import MediaRenderer from './renderers/MediaRenderer';
import PdfRenderer from './renderers/PdfRenderer';

export function PreviewRenderer({ model }: { model: PreviewModel }) {
  switch (model.kind) {
    case 'markdown':
      return <MarkdownRenderer model={model} />;
    case 'json':
      return <JsonRenderer model={model} />;
    case 'code':
      return <CodeRenderer model={model} />;
    case 'image':
    case 'video':
    case 'audio':
      return <MediaRenderer model={model} />;
    case 'pdf':
      return <PdfRenderer model={model} />;
    // HTML lane 未开放（H0 BLOCKED）；T20→T21→T15 完成后在此追加 HtmlRenderer
    case 'html':
    case 'csv':
    case 'archive':
    case 'unsupported':
      return (
        <div data-preview-kind="unsupported" style={{ padding: 24, color: 'var(--text-secondary)' }}>
          {model.kind === 'unsupported' ? model.reason : `预览类型 ${model.kind} 尚未启用`}
        </div>
      );
    default: {
      const _exhaustive: never = model;
      return <div data-preview-kind="unsupported">未知预览类型</div>;
    }
  }
}
