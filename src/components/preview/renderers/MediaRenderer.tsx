'use client';

/**
 * T14 · MediaRenderer — 消费 { kind: 'image' | 'video' | 'audio' } PreviewModel。
 * 只渲染已授权 asset URL（model.src 由 provider 在 authorizeFile 后生成）。
 * 不注入事件副作用（R-P3）。
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
    default:
      return (
<<<<<<< HEAD
=======
        // eslint-disable-next-line @next/next/no-img-element
>>>>>>> agent/resource-preview-v2/20260808-175539/t14-media-pdf
        <img src={model.src} alt={model.name} className="preview-media" data-preview-kind="image" style={{ maxWidth: '100%', objectFit: 'contain' }} />
      );
  }
}
