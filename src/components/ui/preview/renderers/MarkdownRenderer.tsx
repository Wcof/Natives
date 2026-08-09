'use client';

/**
 * T12 · MarkdownRenderer — 只消费 { kind: 'markdown' } PreviewModel。
 * loading/error/fallback 由外层 PreviewSurface 负责；本组件不决定「这个 path 该用谁」。
 * 复用 SafeMarkdown（现有唯一权威 Markdown renderer），按 model.urlPolicy 注入
 * file markdown 的本地图片 rewrite 与已授权 asset URL 放行策略。
 */

import SafeMarkdown from '@/components/ui/SafeMarkdown';
import type { PreviewModel } from '@/lib/preview/contracts';
import { buildMarkdownRenderOptions } from '@/lib/preview/providers/markdown-policy';

export type MarkdownModel = Extract<PreviewModel, { kind: 'markdown' }>;

export default function MarkdownRenderer({ model }: { model: MarkdownModel }) {
  const opts = buildMarkdownRenderOptions(model.urlPolicy, model.baseDir);
  const source = opts.rewrite ? opts.rewrite(model.source) : model.source;
  return <SafeMarkdown source={source} urlTransform={opts.urlTransform} />;
}
