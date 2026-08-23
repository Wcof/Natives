'use client';

/**
 * AnimatedMetric —— 数值变化时做克制补间动画。
 * 尊重 prefers-reduced-motion（直接跳过动画）。
 * 灵感来自 Midday 数字平滑过渡思想，完全自研实现（无源码复制）。
 */

import { useEffect, useRef, useState } from 'react';
import { useSystemReducedMotion } from './ds-utils';

export interface AnimatedMetricProps {
  /** 数值 */
  value: number;
  format?: (value: number) => string;
  prefix?: string;
  suffix?: string;
  /** 补间时长 ms（默认 400，克制）。 */
  duration?: number;
  className?: string;
  style?: React.CSSProperties;
  /** 变动趋势颜色提示（绿色上行 / 红色下行） */
  showTrendColor?: boolean;
}

export function AnimatedMetric({
  value,
  format,
  prefix = '',
  suffix = '',
  duration = 400,
  className,
  style,
  showTrendColor = false,
}: AnimatedMetricProps) {
  const reduced = useSystemReducedMotion();
  const [display, setDisplay] = useState(value);
  const [trend, setTrend] = useState<'up' | 'down' | null>(null);
  const fromRef = useRef(value);

  useEffect(() => {
    if (reduced) {
      setDisplay(value);
      fromRef.current = value;
      return;
    }

    const from = fromRef.current;
    if (from === value) return;

    if (value > from) {
      setTrend('up');
    } else if (value < from) {
      setTrend('down');
    }

    const start = performance.now();
    let raf = 0;

    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      // Quintic ease-out for ultra smooth mechanical feeling
      const eased = 1 - Math.pow(1 - t, 4);
      const next = from + (value - from) * eased;
      setDisplay(next);
      if (t < 1) {
        raf = requestAnimationFrame(tick);
      } else {
        fromRef.current = value;
        const timer = setTimeout(() => setTrend(null), 1000);
        return () => clearTimeout(timer);
      }
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value, duration, reduced]);

  const formattedNumber =
    format !== undefined ? format(display) : Math.round(display).toLocaleString();

  const trendClass =
    showTrendColor && trend === 'up'
      ? 'text-[var(--success)] transition-colors'
      : showTrendColor && trend === 'down'
        ? 'text-[var(--danger)] transition-colors'
        : '';

  return (
    <span
      className={[className, trendClass].filter(Boolean).join(' ')}
      style={{ fontVariantNumeric: 'tabular-nums', ...style }}
    >
      {prefix}
      {formattedNumber}
      {suffix}
    </span>
  );
}

export default AnimatedMetric;
