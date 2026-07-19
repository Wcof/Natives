'use client';

import React, { useMemo, useState } from 'react';
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
import { fmtCount } from '@/lib/format';

interface Props {
  daily: UsageDailyRecord[];
  activity: UsageActivityBucket[];
  sessions: UsageSessionRecord[];
  sources: UsageSourceStatus[];
  metrics: UsageMetrics | null;
  lastRefresh: number | null;
}

export function UsageCharts({ daily, activity, sessions, sources, metrics, lastRefresh }: Props) {
  const locale = useLocale();
  const numberLocale = locale.startsWith('zh') ? 'zh-CN' : 'en-US';
  const compact = (value: number) => fmtCount(value, locale);
  const exact = (value: unknown) => Number(value ?? 0).toLocaleString(numberLocale);

  const trend = useMemo(() => buildDailyTrend(daily, activity), [daily, activity]);
  const heatmap = useMemo(() => buildHourlyHeatmap(activity), [activity]);
  const othersLabel = t(locale, 'usage.others');
  const sourceDist = useMemo(() => buildSourceDistribution(daily, sources, othersLabel), [daily, sources, othersLabel]);
  const modelDist = useMemo(() => buildModelDistribution(daily, othersLabel), [daily, othersLabel]);
  const projectDist = useMemo(() => {
    const dist = buildProjectDistribution(daily, othersLabel);
    return dist.map((p) => {
      const isPath = p.label.includes('/') || p.label.includes('\\');
      if (!isPath) return p;
      const cleaned = p.label.replace(/[/\\]+$/, '');
      const lastSlash = Math.max(cleaned.lastIndexOf('/'), cleaned.lastIndexOf('\\'));
      const basename = lastSlash === -1 ? cleaned : cleaned.substring(lastSlash + 1);
      return { ...p, label: basename || p.label };
    });
  }, [daily, othersLabel]);

  // Check if any source supports hourly data
  const hasHourlySources = sources.some((s) => s.capabilities.hourly);

  // Switcher states
  const [trendMetric, setTrendMetric] = useState<'token' | 'cost' | 'duration'>('token');
  const [heatmapMetric, setHeatmapMetric] = useState<'token' | 'duration'>('token');

  // Heatmap grid mapping
  const grid = useMemo(() => {
    const arr = Array(7).fill(0).map(() => Array(24).fill(0));
    let maxVal = 0;
    for (const p of heatmap) {
      const val = heatmapMetric === 'token' ? (p.totalTokens ?? 0) : (p.activeSeconds ?? 0);
      if (val > maxVal) maxVal = val;
      const row = p.dayOfWeek === 0 ? 6 : p.dayOfWeek - 1;
      if (row >= 0 && row < 7 && p.hour >= 0 && p.hour < 24) {
        const rowArr = arr[row];
        if (rowArr) {
          rowArr[p.hour] = val;
        }
      }
    }
    return { matrix: arr, maxVal };
  }, [heatmap, heatmapMetric]);

  const DAY_LABELS = [
    t(locale, 'usage.dayMon'), t(locale, 'usage.dayTue'), t(locale, 'usage.dayWed'),
    t(locale, 'usage.dayThu'), t(locale, 'usage.dayFri'), t(locale, 'usage.daySat'), t(locale, 'usage.daySun'),
  ];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.md }}>
      {/* ── Charts Row ── */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: SPACING.md }}>
        {/* Token Trend */}
        <div style={{ padding: SPACING.md, borderRadius: BORDER_RADIUS.md, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: SPACING.sm }}>
            <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)' }}>
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
            <ResponsiveContainer width="100%" height={180}>
              {trendMetric === 'token' ? (
                <BarChart data={trend}>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                  <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                  <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8, fontSize: 11 }} formatter={exact} />
                  <Bar dataKey="inputTokens" stackId="a" fill="var(--chart-volume-2)" name={t(locale, 'usage.inputTokens')} />
                  <Bar dataKey="cacheReadTokens" stackId="a" fill="var(--chart-volume-5)" name={t(locale, 'usage.cacheRead')} />
                  <Bar dataKey="outputTokens" stackId="a" fill="var(--chart-volume-8)" name={t(locale, 'usage.outputTokens')} />
                </BarChart>
              ) : trendMetric === 'cost' ? (
                <BarChart data={trend}>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                  <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: number) => `$${v.toFixed(2)}`} />
                  <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8, fontSize: 11 }} formatter={(v: any) => [`$${Number(v).toFixed(4)}`, t(locale, 'usage.costLabel')]} />
                  <Bar dataKey="costUsd" fill="var(--chart-volume-7)" radius={[2, 2, 0, 0]} name={t(locale, 'usage.costLabel')} />
                </BarChart>
              ) : (
                <BarChart data={trend.map(t_ => ({ ...t_, activeMinutes: (t_.activeSeconds ?? 0) / 60 }))}>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                  <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} />
                  <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8, fontSize: 11 }} formatter={(v: any) => [`${Number(v).toFixed(1)} ${t(locale, 'usage.minutes')}`, t(locale, 'usage.estimatedActiveDuration')]} />
                  <Bar dataKey="activeMinutes" fill="var(--chart-volume-7)" radius={[2, 2, 0, 0]} name={t(locale, 'usage.estimatedActiveDuration')} />
                </BarChart>
              )}
            </ResponsiveContainer>
          ) : (
            <EmptyState icon={<BarChart3 size={20} />} title={t(locale, 'usage.noTrendData')} description="" />
          )}
        </div>

        {/* Hourly Heatmap (GitHub-style 7x24 Grid) */}
        <div style={{ padding: SPACING.md, borderRadius: BORDER_RADIUS.md, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)', display: 'flex', flexDirection: 'column' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: SPACING.sm }}>
            <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)' }}>
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
            <div style={{ flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'center' }}>
              <div style={{ display: 'flex', gap: 8 }}>
                {/* Day labels */}
                <div style={{ display: 'flex', flexDirection: 'column', gap: 3, justifyContent: 'space-between', paddingBottom: 14 }}>
                  {DAY_LABELS.map((day) => (
                    <span key={day} style={{ fontSize: '9px', color: 'var(--text-dim)', height: 11, display: 'flex', alignItems: 'center', minWidth: 20 }}>
                      {day}
                    </span>
                  ))}
                </div>
                
                {/* Grid */}
                <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                  {grid.matrix.map((row, rIdx) => (
                    <div key={rIdx} style={{ display: 'flex', gap: 3 }}>
                      {row.map((val, cIdx) => {
                        const hasVal = val > 0;
                        const level = getChartVolumeLevel(val, grid.maxVal);
                        const colorVar = `var(--chart-volume-${level})`;
                        const tooltip = `${DAY_LABELS[rIdx]} ${cIdx.toString().padStart(2, '0')}:00 — ${
                          hasVal ? (heatmapMetric === 'token' ? `${val.toLocaleString(numberLocale)} ${t(locale, 'usage.tokens')}` : `${Math.round(val / 60)} ${t(locale, 'usage.minutes')}`) : '0'
                        }`;

                        return (
                          <div
                            key={cIdx}
                            title={tooltip}
                            style={{
                              flex: 1,
                              aspectRatio: '1',
                              borderRadius: '2px',
                              background: colorVar,
                              cursor: 'pointer',
                              transition: 'transform 0.1s ease',
                            }}
                            onMouseEnter={(e) => {
                              e.currentTarget.style.transform = 'scale(1.3)';
                              e.currentTarget.style.zIndex = '10';
                            }}
                            onMouseLeave={(e) => {
                              e.currentTarget.style.transform = 'none';
                              e.currentTarget.style.zIndex = '1';
                            }}
                          />
                        );
                      })}
                    </div>
                  ))}
                  
                  {/* Hour labels */}
                  <div style={{ display: 'flex', position: 'relative', marginTop: 2, height: 12 }}>
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
            </div>
          ) : (
            <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <EmptyState icon={<Clock size={20} />} title={t(locale, 'usage.noHourlyActivity')} description="" />
            </div>
          )}
        </div>
      </div>

      {/* ── Distribution Row ── */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: SPACING.md }}>
        {/* Source Distribution */}
        <div style={{ padding: SPACING.md, borderRadius: BORDER_RADIUS.md, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)', marginBottom: SPACING.sm }}>{t(locale, 'usage.sourceDistribution')}</div>
          {sourceDist.length > 0 ? (
            <ResponsiveContainer width="100%" height={180}>
              <BarChart data={sourceDist} layout="vertical">
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={60} />
                <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8 }} formatter={exact} />
                <Bar dataKey="totalTokens" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')}>
                  {sourceDist.map((entry, index) => {
                    const maxVal = Math.max(...sourceDist.map(d => d.totalTokens ?? 0));
                    const level = getChartVolumeLevel(entry.totalTokens ?? 0, maxVal);
                    return <Cell key={`cell-${index}`} fill={`var(--chart-volume-${level})`} />;
                  })}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          ) : (
            <EmptyState icon={<PieChart size={20} />} title={t(locale, 'usage.noSourceData')} description="" />
          )}
        </div>

        {/* Model Distribution */}
        <div style={{ padding: SPACING.md, borderRadius: BORDER_RADIUS.md, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)', marginBottom: SPACING.sm }}>{t(locale, 'usage.modelDistribution')}</div>
          {modelDist.length > 0 ? (
            <ResponsiveContainer width="100%" height={180}>
              <BarChart data={modelDist.map((m) => ({ ...m, label: m.modelId === null ? t(locale, 'usage.unrecordedModel') : m.modelId }))} layout="vertical">
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={80} />
                <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8 }} formatter={exact} />
                <Bar dataKey="totalTokens" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')}>
                  {modelDist.map((entry, index) => {
                    const maxVal = Math.max(...modelDist.map(d => d.totalTokens ?? 0));
                    const level = getChartVolumeLevel(entry.totalTokens ?? 0, maxVal);
                    return <Cell key={`cell-${index}`} fill={`var(--chart-volume-${level})`} />;
                  })}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          ) : (
            <EmptyState icon={<BarChart3 size={20} />} title={t(locale, 'usage.noModelData')} description="" />
          )}
        </div>

        {/* Project Distribution */}
        <div style={{ padding: SPACING.md, borderRadius: BORDER_RADIUS.md, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)', marginBottom: SPACING.sm }}>{t(locale, 'usage.projectDistribution')}</div>
          {projectDist.length > 0 ? (
            <ResponsiveContainer width="100%" height={180}>
              <BarChart data={projectDist} layout="vertical">
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={compact} />
                <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={80} />
                <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8 }} formatter={exact} />
                <Bar dataKey="totalTokens" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')}>
                  {projectDist.map((entry, index) => {
                    const maxVal = Math.max(...projectDist.map(d => d.totalTokens ?? 0));
                    const level = getChartVolumeLevel(entry.totalTokens ?? 0, maxVal);
                    return <Cell key={`cell-${index}`} fill={`var(--chart-volume-${level})`} />;
                  })}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          ) : (
            <EmptyState icon={<Folder size={20} />} title={t(locale, 'usage.noProjectData')} description="" />
          )}
        </div>
      </div>
    </div>
  );
}
