'use client';

/** 任务模块通用状态徽标 — 只呈现真实状态文本，无动画/无假进度（R-F2） */

export type JobBadgeTone = 'success' | 'danger' | 'warning' | 'neutral' | 'info';

const TONE_COLOR: Record<JobBadgeTone, string> = {
  success: 'var(--success)',
  danger: 'var(--danger)',
  warning: 'var(--warning)',
  info: 'var(--primary)',
  neutral: 'var(--text-disabled)',
};

export default function JobStatusBadge({
  tone,
  label,
  title,
}: {
  tone: JobBadgeTone;
  label: string;
  title?: string;
}) {
  const color = TONE_COLOR[tone];
  return (
    <span
      title={title}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        padding: '1px 8px',
        borderRadius: 999,
        border: `1px solid ${color}`,
        color,
        fontSize: '0.6875rem',
        fontWeight: 500,
        lineHeight: '1.2rem',
        whiteSpace: 'nowrap',
      }}
    >
      {label}
    </span>
  );
}
