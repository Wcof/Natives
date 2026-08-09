/**
 * 统一格式化工具（对齐 Natives2 风格）
 */

/** 文件大小格式化：< 10 时保留一位小数，否则取整 */
export function fmtSize(n: number): string {
  if (!n) return '';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

import { t } from '@/i18n';

/** 相对时间格式化（中文风格） */
export function fmtTime(ms: number, locale = 'zh'): string {
  if (!ms) return '';
  const diff = Date.now() - ms;
  if (diff < 60_000) return t(locale, 'format.justNow');
  if (diff < 3_600_000) return t(locale, 'format.minutesAgo', { n: Math.floor(diff / 60_000) });
  if (diff < 86_400_000) return t(locale, 'format.hoursAgo', { n: Math.floor(diff / 3_600_000) });
  if (diff < 604_800_000) return t(locale, 'format.daysAgo', { n: Math.floor(diff / 86_400_000) });
  const d = new Date(ms);
  const p = (x: number) => String(x).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** 绝对时间格式化：YYYY-MM-DD HH:mm */
export function fmtDateTime(ms: number): string {
  if (!ms) return '—';
  const d = new Date(ms);
  const p = (x: number) => String(x).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** 数字格式化（用于统计卡片） */
export function fmtCount(n: number, locale: string = 'en'): string {
  return new Intl.NumberFormat(locale.startsWith('zh') ? 'zh-CN' : 'en-US', {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(n);
}

/** 时长格式化（秒 → 可读文本） */
export function fmtDuration(seconds: number, locale = 'en'): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return t(locale, 'format.durationSeconds', { s });
  if (s < 3600) {
    const m = Math.floor(s / 60);
    const rem = s % 60;
    return rem > 0 ? t(locale, 'format.durationMinutesSeconds', { m, rem }) : t(locale, 'format.durationMinutes', { m });
  }
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return m > 0 ? t(locale, 'format.durationHoursMinutes', { h, m }) : t(locale, 'format.durationHours', { h });
}

/**
 * Compact duration for dense UI (heatmap tooltips / chart ticks).
 * Avoids the old `Math.round(sec/60)` trap that turns 1–29s into "0 min".
 */
export function fmtDurationCompact(seconds: number, locale = 'en'): string {
  const s = Math.max(0, Math.round(seconds));
  if (s <= 0) return t(locale, 'format.durationZero');
  if (s < 60) return t(locale, 'format.durationSeconds', { s });
  if (s < 3600) {
    const m = Math.floor(s / 60);
    const rem = s % 60;
    if (rem === 0) return t(locale, 'format.durationMinutes', { m });
    return t(locale, 'format.durationMinutesSeconds', { m, rem });
  }
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (m === 0) return t(locale, 'format.durationHours', { h });
  return t(locale, 'format.durationHoursMinutes', { h, m });
}
