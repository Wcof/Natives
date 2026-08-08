'use client';

/**
 * T14 · PdfRenderer — 消费 { kind: 'pdf' } PreviewModel。
 * iframe 沙箱不含 allow-same-origin / allow-top-navigation / allow-popups（R-S2）。
 * src 是已授权 asset URL（provider 在 authorizeFile 后生成）。
 */

import type { PreviewModel } from '@/lib/preview/contracts';

export type PdfModel = Extract<PreviewModel, { kind: 'pdf' }>;

export default function PdfRenderer({ model }: { model: PdfModel }) {
  return (
    <iframe
      src={model.src}
      title={model.name}
      sandbox="allow-scripts"
      className="preview-pdf"
      data-preview-kind="pdf"
      style={{ width: '100%', height: '100%', border: 0 }}
    />
  );
}
