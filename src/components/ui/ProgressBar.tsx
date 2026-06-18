'use client';

/**
 * 通用进度条组件 — 复用自 UsagePanel 和 Dashboard
 * 对齐 fanbox usagePanel.bar() 样式
 */

export function ProgressBar({ label, used, limit, color }: {
  label: string; used: number; limit: number; color: string;
}) {
  const pct = limit > 0 ? Math.min(100, (used / limit) * 100) : 0;
  const isDanger = pct >= 85;
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, marginBottom: 4 }}>
        <span style={{ color: 'var(--text-dim)' }}>{label}</span>
        <span style={{ color: isDanger ? 'var(--danger)' : 'var(--text)', fontWeight: isDanger ? 700 : undefined }}>
          {Math.round(pct)}% ({used}/{limit})
        </span>
      </div>
      <div style={{ height: 4, background: 'var(--vibe-btn-bg)', borderRadius: 3, overflow: 'hidden' }}>
        <div style={{
          width: `${pct}%`, height: '100%',
          background: isDanger ? 'var(--danger)' : color,
          borderRadius: 3, transition: 'width 0.3s ease',
        }} />
      </div>
    </div>
  );
}

export function TokenChip({ value, label }: { value: number; label: string }) {
  return (
    <span style={{
      flex: 1, padding: '4px 8px', borderRadius: 6,
      background: 'var(--vibe-btn-bg)', fontSize: 11, textAlign: 'center',
      fontFamily: 'var(--font-mono)', color: 'var(--text)',
    }}>
      {value.toLocaleString()} <span style={{ color: 'var(--text-faint)', fontSize: 9 }}>{label}</span>
    </span>
  );
}
