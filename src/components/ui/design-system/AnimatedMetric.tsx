'use client';

/**
 * AnimatedMetric —— 数值变化时做克制补间动画。
 * 尊重 prefers-reduced-motion（直接跳过动画）。
 */

import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { useSystemReducedMotion } from './ds-utils';

export interface AnimatedMetricProps {
  /** 数值（number 或 string/ReactNode 原样输出）。 */
  value: number;
  format?: (value: number) => string;
  /** 补间时长 ms（默认 400，克制）。 */
  duration?: number;
  className?: string;
  style?: React.CSSProperties;
}

export function AnimatedMetric({ value, format, duration = 400, className, style }: AnimatedMetricProps) {
  const reduced = useSystemReducedMotion();
  const [display, setDisplay] = useState(value);
  const fromRef = useRef(value);

  useEffect(() => {
    if (reduced) {
      setDisplay(value);
      fromRef.current = value;
      return;
    }

    const from = fromRef.current;
    if (from === value) return;
    const start = performance.now();
    let raf = 0;

    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      const next = from + (value - from) * eased;
      setDisplay(next);
      if (t < 1) {
        raf = requestAnimationFrame(tick);
      } else {
        fromRef.current = value;
      }
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value, duration, reduced]);

  const rendered: ReactNode =
    format !== undefined ? format(display) : String(Math.round(display));

  return (
    <span className={className} style={{ fontVariantNumeric: 'tabular-nums', ...style }}>
      {rendered}
    </span>
  );
}

export default AnimatedMetric;
