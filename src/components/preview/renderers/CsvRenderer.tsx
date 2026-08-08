'use client';

/**
 * T16 · CsvRenderer — 只消费 { kind: 'csv' } PreviewModel。
 * 纯展示：headers/rows 来自 provider 解析结果，本组件不做 IO/parse。
 * 有界渲染：超出预算时显式 truncated 提示，不创建无界 DOM（R-P4）。
 */

<<<<<<< HEAD
import { t, useLocale } from '@/i18n';
=======
>>>>>>> agent/resource-preview-v2/20260808-175539/t16-csv
import type { PreviewModel } from '@/lib/preview/contracts';

export type CsvModel = Extract<PreviewModel, { kind: 'csv' }>;

export default function CsvRenderer({ model }: { model: CsvModel }) {
<<<<<<< HEAD
  const locale = useLocale();
=======
>>>>>>> agent/resource-preview-v2/20260808-175539/t16-csv
  return (
    <div data-preview-kind="csv" style={{ overflow: 'auto', padding: 12 }}>
      {model.truncated && (
        <div style={{ padding: '4px 0 8px', color: 'var(--text-secondary)', fontSize: 12 }}>
<<<<<<< HEAD
          {t(locale, 'preview.csvTruncated', { count: model.rows.length })}
=======
          CSV 数据较大，已截断展示前 {model.rows.length} 行
>>>>>>> agent/resource-preview-v2/20260808-175539/t16-csv
        </div>
      )}
      <table style={{ borderCollapse: 'collapse', width: '100%', fontSize: 13 }}>
        <thead>
          <tr>
            {model.headers.map((h, i) => (
              <th key={i} style={{ textAlign: 'left', borderBottom: '1px solid var(--border)', padding: '4px 8px', fontWeight: 600 }}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {model.rows.map((row, r) => (
            <tr key={r}>
              {row.map((cell, c) => (
                <td key={c} style={{ borderBottom: '1px solid var(--border)', padding: '4px 8px', whiteSpace: 'nowrap', maxWidth: 320, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
