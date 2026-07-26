'use client';

import React, { useMemo, useState, useCallback, useEffect, useRef } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import type { UsageDailyRecord, UsageActivityBucket, UsageSessionRecord, UsageMetrics, UsageSourceStatus } from '@/types/usage';
import {
  buildDailyTrend, buildHourlyHeatmap, buildSourceDistribution, buildModelDistribution,
  buildProjectDistribution, getChartVolumeLevel,
} from '@/lib/usage-dashboard';
import {
  BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, CartesianGrid, Cell,
} from 'recharts';
import { EmptyState } from '@/components/ui/EmptyState';
import { BarChart3, Clock, Folder, PieChart } from 'lucide-react';
import { fmtCount, fmtDurationCompact } from '@/lib/format';
import styles from './UsageDashboard.module.css';

interface Props {
  daily: UsageDailyRecord[];
  activity: UsageActivityBucket[];
  sessions: UsageSessionRecord[];
  sources: UsageSourceStatus[];
  metrics: UsageMetrics | null;
  lastRefresh: number | null;
}

/** Shared floating panel style for chart hover details (Recharts + heatmap). */
const tipPanelStyle: React.CSSProperties = {
  background: 'var(--bg)',
  border: '0.0625rem solid var(--border)',
  borderRadius: 8,
  padding: '8px 10px',
  fontSize: 11,
  color: 'var(--text)',
  boxShadow: '0 8px 24px color-mix(in srgb, var(--text) 12%, transparent)',
  maxWidth: 260,
  lineHeight: 1.45,
  pointerEvents: 'none',
};

function TipRow({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, marginTop: 2 }}>
      <span style={{ color: color ?? 'var(--text-dim)', display: 'inline-flex', alignItems: 'center', gap: 6 }}>
        {color ? (
          <span
            style={{
              width: 7,
              height: 7,
              borderRadius: 999,
              background: color,
              flex: '0 0 auto',
            }}
          />
        ) : null}
        {label}
      </span>
      <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, color: 'var(--text)' }}>{value}</span>
    </div>
  );
}

export function UsageCharts({ daily, activity, sessions, sources, metrics, lastRefresh }: Props) {
  const locale = useLocale();
  const numberLocale = locale.startsWith('zh') ? 'zh-CN' : 'en-US';
  const compact = (value: number) => fmtCount(value, locale);
  const exact = useCallback(
    (value: unknown) => Number(value ?? 0).toLocaleString(numberLocale),
    [numberLocale],
  );
  const pct = useCallback(
    (value: number | null | undefined) =>
      value == null || !Number.isFinite(value) ? '—' : `${(value * 100).toFixed(1)}%`,
    [],
  );

  const trend = useMemo(() => buildDailyTrend(daily, activity), [daily, activity]);
  const heatmap = useMemo(() => buildHourlyHeatmap(activity), [activity]);
  const othersLabel = t(locale, 'usage.others');
  const sourceDist = useMemo(() => buildSourceDistribution(daily, sources, othersLabel), [daily, sources, othersLabel]);
  const modelDist = useMemo(() => {
    return buildModelDistribution(daily, othersLabel).map((m) => ({
      ...m,
      label: m.modelId === null ? t(locale, 'usage.unrecordedModel') : m.modelId,
    }));
  }, [daily, othersLabel, locale]);
  const projectDist = useMemo(() => {
    const dist = buildProjectDistribution(daily, othersLabel);
    return dist.map((p) => {
      const isPath = p.label.includes('/') || p.label.includes('\\');
      if (!isPath) return p;
      const cleaned = p.label.replace(/[/\\]+$/, '');
      const lastSlash = Math.max(cleaned.lastIndexOf('/'), cleaned.lastIndexOf('\\'));
      const basename = lastSlash === -1 ? cleaned : cleaned.substring(lastSlash + 1);
      return { ...p, label: basename || p.label, fullLabel: p.label };
    });
  }, [daily, othersLabel]);

  // Check if any source supports hourly data
  const hasHourlySources = sources.some((s) => s.capabilities.hourly);

  // Switcher states
  const [trendMetric, setTrendMetric] = useState<'token' | 'cost' | 'duration'>('token');
  const [heatmapMetric, setHeatmapMetric] = useState<'token' | 'duration'>('token');

  // Heatmap grid mapping + rich hover state
  const grid = useMemo(() => {
    const arr = Array(7).fill(0).map(() => Array(24).fill(0));
    let maxVal = 0;
    let totalVal = 0;
    for (const p of heatmap) {
      const val = heatmapMetric === 'token' ? (p.totalTokens ?? 0) : (p.activeSeconds ?? 0);
      if (val > maxVal) maxVal = val;
      totalVal += val;
      const row = p.dayOfWeek === 0 ? 6 : p.dayOfWeek - 1;
      if (row >= 0 && row < 7 && p.hour >= 0 && p.hour < 24) {
        const rowArr = arr[row];
        if (rowArr) {
          rowArr[p.hour] = (rowArr[p.hour] ?? 0) + val;
        }
      }
    }
    return { matrix: arr, maxVal, totalVal };
  }, [heatmap, heatmapMetric]);

  type HeatHover = {
    row: number;
    hour: number;
    value: number;
    x: number;
    y: number;
  };
  const [heatHover, setHeatHover] = useState<HeatHover | null>(null);
  const heatWrapRef = useRef<HTMLDivElement | null>(null);

  // Dismiss floating tip on scroll/resize so it doesn't stick mid-air.
  useEffect(() => {
    if (!heatHover) return;
    const clear = () => setHeatHover(null);
    window.addEventListener('scroll', clear, true);
    window.addEventListener('resize', clear);
    return () => {
      window.removeEventListener('scroll', clear, true);
      window.removeEventListener('resize', clear);
    };
  }, [heatHover]);

  const DAY_LABELS = [
    t(locale, 'usage.dayMon'), t(locale, 'usage.dayTue'), t(locale, 'usage.dayWed'),
    t(locale, 'usage.dayThu'), t(locale, 'usage.dayFri'), t(locale, 'usage.daySat'), t(locale, 'usage.daySun'),
  ];

  const tipBaseStyle = useMemo(
    () => ({
      ...tipPanelStyle,
      // Recharts injects contentStyle onto its own wrapper; keep both consistent.
    }),
    [],
  );

  /** Recharts default tooltip content — shows label + each series with name, value, share. */
  const renderSeriesTooltip = useCallback(
    (props: any) => {
      const { active, payload, label } = props ?? {};
      if (!active || !payload || payload.length === 0) return null;
      const rows = (payload as any[]).filter((e) => e && e.value != null && e.value !== 0);
      if (rows.length === 0) {
        // Still show zero-state so hover never looks empty.
        return (
          <div style={tipPanelStyle}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>{label}</div>
            <div style={{ color: 'var(--text-dim)' }}>{t(locale, 'usage.tooltipNoValue')}</div>
          </div>
        );
      }
      const stackTotal = rows.reduce((s, e) => s + (Number(e.value) || 0), 0);
      return (
        <div style={tipPanelStyle}>
          <div style={{ fontWeight: 600, marginBottom: 4 }}>{label}</div>
          {rows.map((entry, i) => {
            const v = Number(entry.value) || 0;
            const name = entry.name ?? entry.dataKey ?? '';
            const isMoney = String(entry.dataKey).toLowerCase().includes('cost') || name === t(locale, 'usage.costLabel');
            const isMinutes = String(entry.dataKey).toLowerCase().includes('minute')
              || name === t(locale, 'usage.estimatedActiveDuration');
            let valueText: string;
            if (isMoney) valueText = `$${v.toFixed(4)}`;
            else if (isMinutes) {
              // trend duration series is already in minutes; convert back to seconds for formatter
              valueText = fmtDurationCompact(v * 60, locale);
            } else valueText = exact(v);
            return (
              <TipRow
                key={`${entry.dataKey ?? name}-${i}`}
                label={name}
                value={valueText}
                color={typeof entry.color === 'string' ? entry.color : undefined}
              />
            );
          })}
          {rows.length > 1 && stackTotal > 0 ? (
            <div style={{ marginTop: 6, paddingTop: 6, borderTop: '0.0625rem solid var(--border)' }}>
              <TipRow label={t(locale, 'usage.tooltipTotal')} value={exact(stackTotal)} />
            </div>
          ) : null}
        </div>
      );
    },
    [exact, locale],
  );

  /** Horizontal distribution bars: name + tokens + share of total. */
  const renderDistTooltip = useCallback(
    (props: any, kind: 'source' | 'model' | 'project') => {
      const { active, payload } = props ?? {};
      if (!active || !payload || payload.length === 0) return null;
      const row = payload[0]?.payload ?? {};
      const label =
        kind === 'model'
          ? (row.label ?? row.modelId ?? t(locale, 'usage.unrecordedModel'))
          : (row.label ?? row.sourceId ?? row.id ?? '—');
      const tokens = Number(row.totalTokens ?? payload[0]?.value ?? 0);
      const percentage = typeof row.percentage === 'number' ? row.percentage : null;
      const fullPath = kind === 'project' ? row.fullLabel : null;
      return (
        <div style={tipPanelStyle}>
          <div style={{ fontWeight: 600, marginBottom: 2, wordBreak: 'break-all' }}>{label}</div>
          {fullPath && fullPath !== label ? (
            <div style={{ color: 'var(--text-dim)', fontSize: 10, marginBottom: 4, wordBreak: 'break-all' }}>
              {fullPath}
            </div>
          ) : null}
          <TipRow label={t(locale, 'usage.totalTokens')} value={exact(tokens)} />
          <TipRow label={t(locale, 'usage.tooltipShare')} value={pct(percentage)} />
        </div>
      );
    },
    [exact, locale, pct],
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.md }}>
      {/* ── Charts Row ── */}
      <div className={styles.chartsGrid}>
        {/* Token Trend */}
        <div className={styles.chartCard}>
          <div className={styles.chartHeader}>
            <div className={styles.chartTitle}>
              {t(locale, 'usage.tokenTrend')}
            </div>

            {/* Trend Metric Switcher */}
            <div style={{ display: 'flex', background: 'var(--bg-3)', padding: '2px', borderRadius: '12px', border: '1px solid var(--border)' }}>
              {(['token', 'cost', 'duration'] as const).map((m) => {
                const active = trendMetric === m;
                const label = m === 'token' ? t(locale, 'usage.tokenLabel') : m === 'cost' ? t(locale, 'usage.costLabel') : t(locale, 'usage.durationLabel');
                return (
                  <button
                    key={m}
                    onClick={() => setTrendMetric(m)}
                    style={{
                      padding: '2px 8px',
                      borderRadius: '8px',
                      fontSize: '10px',
                      border: 'none',
                      background: active ? 'var(--text)' : 'var(--surface-hover)',
                      color: active ? 'var(--bg)' : 'var(--text-secondary)',
                      cursor: 'pointer',
                      fontWeight: active ? 600 : 400,
                      transition: 'all 0.1s ease',
                    }}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </div>

          {trend.length > 0 ? (
            <div className={styles.chartBody}>
              <ResponsiveContainer width="100%" height="100%">
                {trendMetric === 'token' ? (
                  <BarChart data={trend}>
                    <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                    <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                    <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                    <Tooltip
                      content={renderSeriesTooltip}
                      cursor={{ fill: 'color-mix(in srgb, var(--text) 6%, transparent)' }}
                      wrapperStyle={{ outline: 'none', zIndex: 20 }}
                      contentStyle={tipBaseStyle}
                    />
                    <Bar dataKey="inputTokens" stackId="a" fill="var(--chart-volume-2)" name={t(locale, 'usage.inputTokens')} />
                    <Bar dataKey="cacheReadTokens" stackId="a" fill="var(--chart-volume-5)" name={t(locale, 'usage.cacheRead')} />
                    <Bar dataKey="outputTokens" stackId="a" fill="var(--chart-volume-8)" name={t(locale, 'usage.outputTokens')} />
                  </BarChart>
                ) : trendMetric === 'cost' ? (
                  <BarChart data={trend}>
                    <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                    <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                    <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: number) => `$${v.toFixed(2)}`} />
                    <Tooltip
                      content={renderSeriesTooltip}
                      cursor={{ fill: 'color-mix(in srgb, var(--text) 6%, transparent)' }}
                      wrapperStyle={{ outline: 'none', zIndex: 20 }}
                      contentStyle={tipBaseStyle}
                    />
                    <Bar dataKey="costUsd" fill="var(--chart-volume-7)" radius={[2, 2, 0, 0]} name={t(locale, 'usage.costLabel')} />
                  </BarChart>
                ) : (
                  <BarChart data={trend.map((t_) => ({ ...t_, activeMinutes: (t_.activeSeconds ?? 0) / 60 }))}>
                    <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                    <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                    <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} />
                    <Tooltip
                      content={renderSeriesTooltip}
                      cursor={{ fill: 'color-mix(in srgb, var(--text) 6%, transparent)' }}
                      wrapperStyle={{ outline: 'none', zIndex: 20 }}
                      contentStyle={tipBaseStyle}
                    />
                    <Bar dataKey="activeMinutes" fill="var(--chart-volume-7)" radius={[2, 2, 0, 0]} name={t(locale, 'usage.estimatedActiveDuration')} />
                  </BarChart>
                )}
              </ResponsiveContainer>
            </div>
          ) : (
            <div className={styles.chartBody}>
              <EmptyState icon={<BarChart3 size={20} />} title={t(locale, 'usage.noTrendData')} description="" />
            </div>
          )}
        </div>

        {/* Hourly Heatmap (GitHub-style 7x24 Grid) */}
        <div
          ref={heatWrapRef}
          className={styles.chartCard}
          style={{
            position: 'relative',
            overflow: 'visible',
          }}
        >
          <div className={styles.chartHeader}>
            <div className={styles.chartTitle}>
              {t(locale, 'usage.hourlyHeatmap')}
            </div>

            {/* Heatmap Metric Switcher */}
            <div style={{ display: 'flex', background: 'var(--bg-3)', padding: '2px', borderRadius: '12px', border: '1px solid var(--border)' }}>
              {(['token', 'duration'] as const).map((m) => {
                const active = heatmapMetric === m;
                const label = m === 'token' ? t(locale, 'usage.tokenLabel') : t(locale, 'usage.durationLabel');
                return (
                  <button
                    key={m}
                    onClick={() => setHeatmapMetric(m)}
                    style={{
                      padding: '2px 8px',
                      borderRadius: '8px',
                      fontSize: '10px',
                      border: 'none',
                      background: active ? 'var(--text)' : 'var(--surface-hover)',
                      color: active ? 'var(--bg)' : 'var(--text-secondary)',
                      cursor: 'pointer',
                      fontWeight: active ? 600 : 400,
                      transition: 'all 0.1s ease',
                    }}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </div>

          {heatmap.length > 0 && hasHourlySources ? (
            <div className={styles.chartBody} style={{ alignItems: 'stretch' }}>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3, justifyContent: 'center' }}>
                {grid.matrix.map((row, rIdx) => (
                  <div key={rIdx} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    {/* Day label directly aligned with row */}
                    <span
                      style={{
                        fontSize: '9px',
                        color: 'var(--text-dim)',
                        minWidth: 22,
                        flexShrink: 0,
                        textAlign: 'right',
                        userSelect: 'none',
                      }}
                    >
                      {DAY_LABELS[rIdx]}
                    </span>

                    {/* Row Cells */}
                    <div style={{ flex: 1, display: 'flex', gap: 3 }}>
                      {row.map((val, cIdx) => {
                        const level = getChartVolumeLevel(val, grid.maxVal);
                        const colorVar = `var(--chart-volume-${level})`;

                        return (
                          <div
                            key={cIdx}
                            role="img"
                            aria-label={`${DAY_LABELS[rIdx]} ${cIdx.toString().padStart(2, '0')}:00`}
                            style={{
                              flex: 1,
                              aspectRatio: '1',
                              borderRadius: '2px',
                              background: colorVar,
                              cursor: 'pointer',
                              transition: 'transform 0.1s ease',
                              position: 'relative',
                              zIndex: heatHover?.row === rIdx && heatHover?.hour === cIdx ? 5 : 1,
                            }}
                            onMouseEnter={(e) => {
                              e.currentTarget.style.transform = 'scale(1.35)';
                              const rect = e.currentTarget.getBoundingClientRect();
                              setHeatHover({
                                row: rIdx,
                                hour: cIdx,
                                value: val,
                                x: rect.left + rect.width / 2,
                                y: rect.top,
                              });
                            }}
                            onMouseMove={(e) => {
                              const rect = e.currentTarget.getBoundingClientRect();
                              setHeatHover({
                                row: rIdx,
                                hour: cIdx,
                                value: val,
                                x: rect.left + rect.width / 2,
                                y: rect.top,
                              });
                            }}
                            onMouseLeave={(e) => {
                              e.currentTarget.style.transform = 'none';
                              setHeatHover((cur) =>
                                cur && cur.row === rIdx && cur.hour === cIdx ? null : cur,
                              );
                            }}
                          />
                        );
                      })}
                    </div>
                  </div>
                ))}

                {/* Hour labels */}
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 2, height: 12 }}>
                  <div style={{ minWidth: 22, flexShrink: 0 }} />
                  <div style={{ flex: 1, position: 'relative', height: 12 }}>
                    {[0, 6, 12, 18, 23].map((hour) => {
                      const leftPercent = (hour / 23) * 100;
                      return (
                        <span
                          key={hour}
                          style={{
                            position: 'absolute',
                            left: `${leftPercent}%`,
                            transform: 'translateX(-50%)',
                            fontSize: '9px',
                            color: 'var(--text-dim)',
                            fontFamily: 'var(--font-mono)',
                          }}
                        >
                          {hour.toString().padStart(2, '0')}
                        </span>
                      );
                    })}
                  </div>
                </div>
              </div>

              {/* Floating heatmap detail tip (native title is unreliable in Tauri webviews). */}
              {heatHover ? (
                <div
                  style={{
                    ...tipPanelStyle,
                    position: 'fixed',
                    left: Math.min(
                      Math.max(heatHover.x, 80),
                      (typeof window !== 'undefined' ? window.innerWidth : 800) - 80,
                    ),
                    top: Math.max(heatHover.y - 8, 8),
                    transform: 'translate(-50%, -100%)',
                    zIndex: 1000,
                  }}
                >
                  <div style={{ fontWeight: 600, marginBottom: 4 }}>
                    {DAY_LABELS[heatHover.row]}{' '}
                    {heatHover.hour.toString().padStart(2, '0')}:00
                    –{((heatHover.hour + 1) % 24).toString().padStart(2, '0')}:00
                  </div>
                  {heatmapMetric === 'token' ? (
                    <TipRow
                      label={t(locale, 'usage.totalTokens')}
                      value={heatHover.value > 0 ? exact(heatHover.value) : '0'}
                    />
                  ) : (
                    <TipRow
                      label={t(locale, 'usage.estimatedActiveDuration')}
                      value={fmtDurationCompact(heatHover.value, locale)}
                    />
                  )}
                  <TipRow
                    label={t(locale, 'usage.tooltipShare')}
                    value={
                      grid.totalVal > 0 && heatHover.value > 0
                        ? pct(heatHover.value / grid.totalVal)
                        : '—'
                    }
                  />
                </div>
              ) : null}
            </div>
          ) : (
            <div className={styles.chartBody}>
              <EmptyState icon={<Clock size={20} />} title={t(locale, 'usage.noHourlyActivity')} description="" />
            </div>
          )}
        </div>
      </div>

      {/* ── Distribution Row ── */}
      <div className={styles.distGrid}>
        {/* Source Distribution */}
        <div className={styles.chartCard}>
          <div className={styles.chartHeader}>
            <div className={styles.chartTitle}>{t(locale, 'usage.sourceDistribution')}</div>
          </div>
          {sourceDist.length > 0 ? (
            <div className={styles.chartBody}>
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={sourceDist} layout="vertical">
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                  <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={60} />
                  <Tooltip
                    content={(p) => renderDistTooltip(p, 'source')}
                    cursor={{ fill: 'color-mix(in srgb, var(--text) 6%, transparent)' }}
                    wrapperStyle={{ outline: 'none', zIndex: 20 }}
                    contentStyle={tipBaseStyle}
                  />
                  <Bar dataKey="totalTokens" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')}>
                    {sourceDist.map((entry, index) => {
                      const maxVal = Math.max(...sourceDist.map((d) => d.totalTokens ?? 0));
                      const level = getChartVolumeLevel(entry.totalTokens ?? 0, maxVal);
                      return <Cell key={`cell-${index}`} fill={`var(--chart-volume-${level})`} />;
                    })}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <div className={styles.chartBody}>
              <EmptyState icon={<PieChart size={20} />} title={t(locale, 'usage.noSourceData')} description="" />
            </div>
          )}
        </div>

        {/* Model Distribution */}
        <div className={styles.chartCard}>
          <div className={styles.chartHeader}>
            <div className={styles.chartTitle}>{t(locale, 'usage.modelDistribution')}</div>
          </div>
          {modelDist.length > 0 ? (
            <div className={styles.chartBody}>
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={modelDist} layout="vertical">
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                  <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={80} />
                  <Tooltip
                    content={(p) => renderDistTooltip(p, 'model')}
                    cursor={{ fill: 'color-mix(in srgb, var(--text) 6%, transparent)' }}
                    wrapperStyle={{ outline: 'none', zIndex: 20 }}
                    contentStyle={tipBaseStyle}
                  />
                  <Bar dataKey="totalTokens" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')}>
                    {modelDist.map((entry, index) => {
                      const maxVal = Math.max(...modelDist.map((d) => d.totalTokens ?? 0));
                      const level = getChartVolumeLevel(entry.totalTokens ?? 0, maxVal);
                      return <Cell key={`cell-${index}`} fill={`var(--chart-volume-${level})`} />;
                    })}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <div className={styles.chartBody}>
              <EmptyState icon={<BarChart3 size={20} />} title={t(locale, 'usage.noModelData')} description="" />
            </div>
          )}
        </div>

        {/* Project Distribution */}
        <div className={styles.chartCard}>
          <div className={styles.chartHeader}>
            <div className={styles.chartTitle}>{t(locale, 'usage.projectDistribution')}</div>
          </div>
          {projectDist.length > 0 ? (
            <div className={styles.chartBody}>
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={projectDist} layout="vertical">
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                  <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={80} />
                  <Tooltip
                    content={(p) => renderDistTooltip(p, 'project')}
                    cursor={{ fill: 'color-mix(in srgb, var(--text) 6%, transparent)' }}
                    wrapperStyle={{ outline: 'none', zIndex: 20 }}
                    contentStyle={tipBaseStyle}
                  />
                  <Bar dataKey="totalTokens" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')}>
                    {projectDist.map((entry, index) => {
                      const maxVal = Math.max(...projectDist.map((d) => d.totalTokens ?? 0));
                      const level = getChartVolumeLevel(entry.totalTokens ?? 0, maxVal);
                      return <Cell key={`cell-${index}`} fill={`var(--chart-volume-${level})`} />;
                    })}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <div className={styles.chartBody}>
              <EmptyState icon={<Folder size={20} />} title={t(locale, 'usage.noProjectData')} description="" />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
