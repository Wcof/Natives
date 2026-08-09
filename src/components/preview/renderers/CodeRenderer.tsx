'use client';

/**
 * T13 · CodeRenderer — 只消费 { kind: 'code' } PreviewModel。
 * 只读代码展示，不提供 Monaco 编辑/保存（写路径归 Editor）。
 * 不再是 <pre> 占位：按需 lazy load Shiki 语法高亮（复用 src/lib/shiki-utils），
 * 有加载兜底（有界纯文本 pre）；provider 已在 read/parse 阶段做行/字节预算，
 * 本组件再对 DOM 行数做展示级上限（CODE_RENDER_MAX_LINES，R-P4）。
 */

import { useEffect, useMemo, useState } from 'react';
import { t, useLocale } from '@/i18n';
import { highlightCode } from '@/lib/shiki-utils';
import type { PreviewModel } from '@/lib/preview/contracts';

export type CodeModel = Extract<PreviewModel, { kind: 'code' }>;

/** 展示预算：单次渲染最多 500 行，避免超大代码块一次性创建无界 DOM（R-P4） */
export const CODE_RENDER_MAX_LINES = 500;

export default function CodeRenderer({ model }: { model: CodeModel }) {
  const locale = useLocale();
  const lines = useMemo(() => model.source.split('\n'), [model.source]);
  const displayLines = useMemo(() => lines.slice(0, CODE_RENDER_MAX_LINES), [lines]);
  const displaySource = displayLines.join('\n');
  const renderTruncated = model.truncated || lines.length > CODE_RENDER_MAX_LINES;

  const [html, setHtml] = useState('');
  useEffect(() => {
    let cancelled = false;
    setHtml('');
    // highlightCode 内部已 lazy import shiki 并在失败时返回纯文本 fallback
    highlightCode(displaySource, model.language)
      .then((highlighted) => {
        if (!cancelled) setHtml(highlighted);
      })
      .catch(() => {
        // 极端的加载失败：保留纯文本 pre（下方 fallback 分支）
        if (!cancelled) setHtml('');
      });
    return () => {
      cancelled = true;
    };
  }, [displaySource, model.language]);

  const truncationNote = renderTruncated ? (
    <div data-preview-truncated="code" style={{ padding: '6px 12px', color: 'var(--text-secondary)', fontSize: 12, borderTop: '1px solid var(--border)' }}>
      {t(locale, 'preview.codeTruncated', { count: displayLines.length })}
    </div>
  ) : null;

  if (html) {
    return (
      <div data-preview-kind="code" data-language={model.language}>
        <div className="code-renderer" style={{ overflow: 'auto' }} dangerouslySetInnerHTML={{ __html: html }} />
        {truncationNote}
      </div>
    );
  }

  // 加载/高亮失败兜底：渲染有界纯文本（source 已由 provider 预算截断）
  return (
    <div data-preview-kind="code" data-language={model.language}>
      <pre
        className="code-renderer"
        style={{ margin: 0, padding: 12, overflow: 'auto', whiteSpace: 'pre-wrap', wordBreak: 'break-word', fontFamily: 'var(--font-mono, monospace)', fontSize: 12, lineHeight: 1.6 }}
      >
        {displaySource}
      </pre>
      {truncationNote}
    </div>
  );
}
