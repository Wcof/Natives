'use client';

/**
 * Design System V2 — primitives barrel.
 * 全部组件消费语义 token，禁止调用方传 hex。
 */

import './design-system.css';

export { Surface } from './Surface';
export type { SurfaceProps, SurfaceRadius } from './Surface';
export { MaterialSurface } from './MaterialSurface';
export type { MaterialSurfaceProps, GlowTone } from './MaterialSurface';
export { CrystalSurface } from './CrystalSurface';
export type { CrystalSurfaceProps } from './CrystalSurface';
export { GlowEdge } from './GlowEdge';
export type { GlowEdgeProps } from './GlowEdge';
export { Panel } from './Panel';
export type { PanelProps } from './Panel';
export { MetricBlock } from './MetricBlock';
export type { MetricBlockProps } from './MetricBlock';
export { AnimatedMetric } from './AnimatedMetric';
export type { AnimatedMetricProps } from './AnimatedMetric';
export { ChartFrame, ChartSkeleton } from './ChartFrame';
export type { ChartFrameProps } from './ChartFrame';
export { ChartAreaGradient } from './ChartAreaGradient';
export type { ChartAreaGradientProps } from './ChartAreaGradient';
export { Skeleton } from './Skeleton';
export type { DsSkeletonProps } from './Skeleton';
export { Empty } from './Empty';
export type { EmptyProps } from './Empty';
export { Error as ErrorPrimitive } from './Error';
export type { ErrorProps } from './Error';
export { useSystemReducedMotion, useSystemReducedTransparency } from './ds-utils';
