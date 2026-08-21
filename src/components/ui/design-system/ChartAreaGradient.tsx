'use client';

/**
 * ChartAreaGradient —— Recharts Area 填充渐变 helper（V-025/V-026）。
 * 消费 chart 语义 token（--chart-line / --chart-area-fill）：
 *   · 浅色（Liquid Crystal）法则 4：area fill 约 15% → 0% 水彩渐变。
 *   · 暗色（Dark Glow）：克制 fill，8% → 0%。
 * 用法：
 *   <AreaChart>
 *     <defs><ChartAreaGradient id="usageFill" /></defs>
 *     <Area ... fill="url(#usageFill)" stroke="var(--chart-line)" />
 *   </AreaChart>
 * 组件不产生任何 hex —— 颜色来自 CSS 变量。
 */

import type { ReactElement } from 'react';

export interface ChartAreaGradientProps {
  /** SVG 渐变 id（Area fill="url(#id)" 引用）。 */
  id: string;
  /** 顶部不透明度（浅色默认 0.15 = 15%）。 */
  fromOpacity?: number;
  /** 底部不透明度（默认 0）。 */
  toOpacity?: number;
  /** 渐变方向（默认 180deg = 自上而下）。 */
  orientation?: 'top-to-bottom' | 'bottom-to-top';
  /** 颜色 token（默认 var(--chart-line)；可换 --chart-strong 等）。 */
  colorVar?: string;
}

export function ChartAreaGradient({
  id,
  fromOpacity = 0.15,
  toOpacity = 0,
  orientation = 'top-to-bottom',
  colorVar = 'var(--chart-line)',
}: ChartAreaGradientProps): ReactElement {
  return (
    <linearGradient
      id={id}
      x1="0"
      y1={orientation === 'top-to-bottom' ? '0' : '1'}
      x2="0"
      y2={orientation === 'top-to-bottom' ? '1' : '0'}
    >
      <stop offset="0%" stopColor={colorVar} stopOpacity={fromOpacity} />
      <stop offset="100%" stopColor={colorVar} stopOpacity={toOpacity} />
    </linearGradient>
  );
}

export default ChartAreaGradient;
