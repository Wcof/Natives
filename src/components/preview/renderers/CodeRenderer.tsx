'use client';

/**
 * T13 · CodeRenderer — 只消费 { kind: 'code' } PreviewModel。
 * 只读代码展示（<pre> + 简单高亮占位），不提供 Monaco 编辑/保存（写路径归 Editor）。
 */

import type { PreviewModel } from '@/lib/preview/contracts';

export type CodeModel = Extract<PreviewModel, { kind: 'code' }>;

export default function CodeRenderer({ model }: { model: CodeModel }) {
  return (
    <pre
      className="code-renderer"
      data-preview-kind="code"
      data-language={model.language}
      style={{ margin: 0, overflow: 'auto', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}
    >
      {model.source}
      {model.truncated ? '\n… (truncated)' : ''}
    </pre>
  );
}
