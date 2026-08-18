'use client';

/** Defensive sink for stale/injected HTML models while H0 is BLOCKED. */

import { t, useLocale } from '@/i18n';
import type { PreviewModel } from '@/lib/preview/contracts';

export type HtmlModel = Extract<PreviewModel, { kind: 'html' }>;

export default function HtmlRenderer(_: { model: HtmlModel }) {
  const locale = useLocale();
  return (
    <div data-preview-kind="html-blocked" role="status">
      {t(locale, 'preview.unsupported')}
    </div>
  );
}
