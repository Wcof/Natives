'use client';

/**
 * T20 · HtmlRenderer — 消费 { kind: 'html' } PreviewModel。
 *
 * HTML lane 垂直链（PREV-001 → P2-01）：provider → PreviewContext.prepareHtml
 * （Host 授权 + /fs/{token}/ 逐资源重写）→ 本 renderer 以 srcDoc 呈现。
 * sandbox 由 provider 冻结（R-S2：allow-scripts allow-forms；无 allow-same-origin
 * / allow-top-navigation / allow-popups），renderer 只透传 model.sandbox，不自行
 * 放宽。仅当模型未携带 html（异常 wire）时回退 previewUrl iframe——生产 provider
 * 始终提供 html，此分支不产生额外安全面。
 */

import type { PreviewModel } from '@/lib/preview/contracts';

export type HtmlModel = Extract<PreviewModel, { kind: 'html' }>;

export default function HtmlRenderer({ model }: { model: HtmlModel }) {
  // iframe 背景透明：HTML 文档自己的样式决定呈现；无 body 背景时透过应用主题。
  if (model.html) {
    return (
      <iframe
        title="html preview"
        srcDoc={model.html}
        sandbox={model.sandbox}
        className="preview-html"
        data-preview-kind="html"
        data-revision={model.revision}
        style={{ width: '100%', height: '100%', border: 0 }}
      />
    );
  }
  // 异常 wire 兜底：仅当 provider 未给出 srcDoc 时使用 previewUrl（仍受同一 sandbox 约束）
  return (
    <iframe
      title="html preview"
      src={model.previewUrl}
      sandbox={model.sandbox}
      className="preview-html"
      data-preview-kind="html"
      style={{ width: '100%', height: '100%', border: 0 }}
    />
  );
}
