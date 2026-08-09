'use client';

/**
 * T16 · CsvRenderer — 只消费 { kind: 'csv' } PreviewModel。
 * 纯展示：headers/rows 来自 provider 解析结果，本组件不做 IO/parse。
 * 有界渲染（R-P4）：行数/列数在展示端再做一级 DOM 上限，超出显式提示，
 * 不创建无界 DOM。预算链：provider（字节/行/列）→ 本组件（DOM 行/列）。
 */

import { t, useLocale } from '@/i18n';
import type { PreviewModel } from '@/lib/preview/contracts';

export type CsvModel = Extract<PreviewModel, { kind: 'csv' }>;

/** 展示预算：最多渲染 200 行（R-P4） */
export const CSV_RENDER_MAX_ROWS = 200;
/** 展示预算：最多渲染 30 列（R-P4） */
export const CSV_RENDER_MAX_COLUMNS = 30;

export default function CsvRenderer({ model }: { model: CsvModel }) {
  const locale = useLocale();
  const rowsCapped = model.rows.length > CSV_RENDER_MAX_ROWS;
  const colsCapped = model.headers.length > CSV_RENDER_MAX_COLUMNS;
  const shownRows = rowsCapped ? model.rows.slice(0, CSV_RENDER_MAX_ROWS) : model.rows;
  const shownHeaders = colsCapped ? model.headers.slice(0, CSV_RENDER_MAX_COLUMNS) : model.headers;

  return (
    <div data-preview-kind="csv" style={{ overflow: 'auto', padding: 12 }}>
      {model.truncated && (
        <div style={{ padding: '4px 0 8px', color: 'var(--text-secondary)', fontSize: 12 }}>
          {t(locale, 'preview.csvTruncated', { count: shownRows.length })}
        </div>
      )}
      {rowsCapped && (
        <div style={{ padding: '4px 0 8px', color: 'var(--text-secondary)', fontSize: 12 }}>
          {t(locale, 'preview.csvMoreRows', { count: CSV_RENDER_MAX_ROWS, total: model.rows.length })}
        </div>
      )}
      {colsCapped && (
        <div style={{ padding: '4px 0 8px', color: 'var(--text-secondary)', fontSize: 12 }}>
          {t(locale, 'preview.csvMoreColumns', { count: CSV_RENDER_MAX_COLUMNS })}
        </div>
      )}
      <table style={{ borderCollapse: 'collapse', width: '100%', fontSize: 13 }}>
        <thead>
          <tr>
            {shownHeaders.map((h, i) => (
              <th key={i} style={{ textAlign: 'left', borderBottom: '1px solid var(--border)', padding: '4px 8px', fontWeight: 600 }}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {shownRows.map((row, r) => (
            <tr key={r}>
              {shownHeaders.map((_, c) => (
                <td key={c} style={{ borderBottom: '1px solid var(--border)', padding: '4px 8px', whiteSpace: 'nowrap', maxWidth: 320, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {row[c] ?? ''}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
