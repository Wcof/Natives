'use client';

/**
 * T19 · PreviewRenderer — 穷尽式 renderer composition（唯一组装者）。
 * 只消费 PreviewModel；不决定「这个 path 该用谁」（那属于 registry/service）。
 * loading/error/fallback 由 PreviewSurface 负责，本组件只负责「模型 → 渲染」。
 * 新 kind 必须在此穷尽分支；html 已由 HtmlRenderer 按 sandbox 呈现（P2-01 垂直链）。
 */

import { t, useLocale } from '@/i18n';
import type { PreviewModel } from '@/lib/preview/contracts';
import MarkdownRenderer from './renderers/MarkdownRenderer';
import JsonRenderer from './renderers/JsonRenderer';
import CodeRenderer from './renderers/CodeRenderer';
import MediaRenderer from './renderers/MediaRenderer';
import PdfRenderer from './renderers/PdfRenderer';
import CsvRenderer from './renderers/CsvRenderer';
import ArchiveRenderer from './renderers/ArchiveRenderer';
import HtmlRenderer from './renderers/HtmlRenderer';

export function PreviewRenderer({ model }: { model: PreviewModel }) {
  const locale = useLocale();
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
    case 'csv':
      return <CsvRenderer model={model} />;
    case 'archive':
      return <ArchiveRenderer model={model} />;
    case 'html':
      return <HtmlRenderer model={model} />;
    case 'unsupported':
      return (
        <div data-preview-kind="unsupported" style={{ padding: 24, color: 'var(--text-secondary)' }}>
          {model.reason || t(locale, 'preview.unsupported')}
        </div>
      );
    default: {
      const _exhaustive: never = model;
      return <div data-preview-kind="unsupported">{t(locale, 'preview.unknownType')}</div>;
    }
  }
}
