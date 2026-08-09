'use client';

/**
 * T14 · MediaRenderer — 消费 { kind: 'image' | 'video' | 'audio' } PreviewModel。
 * 只渲染已授权 asset URL（model.src 由 provider 在 authorizeFile 后生成）。
 * 不注入事件副作用（R-P3）。kind 由 provider 判定（PREV-004：Host kind 优先，
 * 未知扩展名不猜 image → unsupported，不进入本组件）。
 */

import type { PreviewModel } from '@/lib/preview/contracts';

export type MediaModel = Extract<PreviewModel, { kind: 'image' } | { kind: 'video' } | { kind: 'audio' }>;

export default function MediaRenderer({ model }: { model: MediaModel }) {
  switch (model.kind) {
    case 'video':
      return (
        <video src={model.src} controls className="preview-media" data-preview-kind="video" style={{ maxWidth: '100%' }} />
      );
    case 'audio':
      return (
        <audio src={model.src} controls className="preview-media" data-preview-kind="audio" style={{ width: '100%' }} />
      );
    case 'image':
      return (
        <img src={model.src} alt={model.name} className="preview-media" data-preview-kind="image" style={{ maxWidth: '100%', objectFit: 'contain' }} />
      );
    default: {
      const _exhaustive: never = model;
      return <div data-preview-kind="unsupported">unsupported media kind</div>;
    }
  }
}
