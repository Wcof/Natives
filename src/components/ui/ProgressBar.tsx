'use client';

/**
 * 通用进度条组件 — 复用自 UsagePanel 和 Dashboard
 * 对齐 Natives2 usagePanel.bar() 样式
 */
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

export function ProgressBar({ label, used, limit, color }: {
  label: string; used: number; limit: number; color: string;
}) {
  const pct = limit > 0 ? Math.min(100, (used / limit) * 100) : 0;
  const isDanger = pct >= 85;
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: FONT_SIZE.sm, marginBottom: SPACING.xs }}>
        <span style={{ color: 'var(--text-dim)' }}>{label}</span>
        <span style={{ color: isDanger ? 'var(--danger)' : 'var(--text)', fontWeight: isDanger ? 700 : undefined }}>
          {Math.round(pct)}% ({used}/{limit})
        </span>
      </div>
      <div style={{ height: 4, background: 'var(--vibe-btn-bg)', borderRadius: BORDER_RADIUS.sm, overflow: 'hidden' }}>
        <div style={{
          width: `${pct}%`, height: '100%',
          background: isDanger ? 'var(--danger)' : color,
          borderRadius: BORDER_RADIUS.sm, transition: 'width 0.3s ease',
        }} />
      </div>
    </div>
  );
}

export function TokenChip({ value, label }: { value: number; label: string }) {
  return (
    <span style={{
      flex: 1, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.md,
      background: 'var(--vibe-btn-bg)', fontSize: FONT_SIZE.sm, textAlign: 'center',
      fontFamily: 'var(--font-mono)', color: 'var(--text)',
    }}>
      {value.toLocaleString()} <span style={{ color: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}>{label}</span>
    </span>
  );
}
