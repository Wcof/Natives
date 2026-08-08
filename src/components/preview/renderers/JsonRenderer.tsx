'use client';

/**
 * T13 · JsonRenderer — 只消费 { kind: 'json' } PreviewModel。
 * 懒展开树：节点数 > 200 时默认折叠，展开有界（R-P4/R12）；超预算标记 truncated。
 * 不使用 Monaco（Monaco 属于 Editor 写路径）。
 */

import { useMemo, useState } from 'react';
<<<<<<< HEAD
import { t, useLocale, type Locale } from '@/i18n';
=======
>>>>>>> agent/resource-preview-v2/20260808-175539/t13-json-code
import type { PreviewModel } from '@/lib/preview/contracts';

export type JsonModel = Extract<PreviewModel, { kind: 'json' }>;

const MAX_VISIBLE = 200;

function isExpandable(value: unknown): boolean {
  return value !== null && typeof value === 'object';
}

function renderLeaf(value: unknown): string {
  if (value === null) return 'null';
  switch (typeof value) {
    case 'string':
      return JSON.stringify(value);
    case 'boolean':
    case 'number':
      return String(value);
    default:
      return String(value);
  }
}

<<<<<<< HEAD
function JsonNode({ label, value, depth, maxDepth, locale }: {
=======
function JsonNode({ label, value, depth, maxDepth }: {
>>>>>>> agent/resource-preview-v2/20260808-175539/t13-json-code
  label: string | null;
  value: unknown;
  depth: number;
  maxDepth: number;
<<<<<<< HEAD
  locale: Locale;
=======
>>>>>>> agent/resource-preview-v2/20260808-175539/t13-json-code
}) {
  const [open, setOpen] = useState(depth < maxDepth);
  const canExpand = isExpandable(value);

  if (!canExpand) {
    return (
      <div className="json-row" style={{ paddingLeft: depth * 12 }}>
        {label !== null && <span className="json-key">{label}: </span>}
        <span className="json-value">{renderLeaf(value)}</span>
      </div>
    );
  }

  const entries = Array.isArray(value)
    ? (value as unknown[]).map((v, i) => [String(i), v] as const)
    : Object.entries(value as Record<string, unknown>);
  const visible = open ? entries.slice(0, MAX_VISIBLE) : [];
  const overflow = open ? entries.length - MAX_VISIBLE : entries.length;

  return (
    <div className="json-node" style={{ paddingLeft: depth * 12 }}>
      <button type="button" className="json-toggle" onClick={() => setOpen(!open)} aria-expanded={open}>
        {open ? '▾' : '▸'}
      </button>
      {label !== null && <span className="json-key">{label}: </span>}
      <span className="json-type">{Array.isArray(value) ? `Array(${entries.length})` : 'Object'}</span>
      {open &&
<<<<<<< HEAD
        visible.map(([k, v]) => <JsonNode key={k} label={k} value={v} depth={depth + 1} maxDepth={maxDepth} locale={locale} />)}
      {open && overflow > 0 && <div className="json-truncated">{t(locale, 'preview.jsonMore', { count: overflow })}</div>}
=======
        visible.map(([k, v]) => <JsonNode key={k} label={k} value={v} depth={depth + 1} maxDepth={maxDepth} />)}
      {open && overflow > 0 && <div className="json-truncated">… {overflow} more (bounded)</div>}
>>>>>>> agent/resource-preview-v2/20260808-175539/t13-json-code
    </div>
  );
}

export default function JsonRenderer({ model }: { model: JsonModel }) {
<<<<<<< HEAD
  const locale = useLocale();
=======
>>>>>>> agent/resource-preview-v2/20260808-175539/t13-json-code
  const root = useMemo(() => (model.nodeCount > MAX_VISIBLE ? model.value : model.value), [model]);
  void root;
  return (
    <div className="json-tree" data-preview-kind="json">
<<<<<<< HEAD
      {model.truncated && <div className="json-warning">{t(locale, 'preview.jsonTruncated', { count: model.nodeCount })}</div>}
      <JsonNode label={null} value={model.value} depth={0} maxDepth={4} locale={locale} />
=======
      {model.truncated && <div className="json-warning">JSON 超出节点/深度预算，已折叠展示（nodeCount={model.nodeCount}）</div>}
      <JsonNode label={null} value={model.value} depth={0} maxDepth={4} />
>>>>>>> agent/resource-preview-v2/20260808-175539/t13-json-code
    </div>
  );
}
