'use client';

import React, { useMemo, useState } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import type { UsageDailyRecord, UsageActivityBucket, UsageSessionRecord, UsageMetrics, UsageSourceStatus } from '@/types/usage';
import {
  buildDailyTrend, buildHourlyHeatmap, buildSourceDistribution, buildModelDistribution,
} from '@/lib/usage-dashboard';
import {
  BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, CartesianGrid,
} from 'recharts';
import { EmptyState } from '@/components/ui/EmptyState';
import { BarChart3, Clock, PieChart, Calendar } from 'lucide-react';

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

  const trend = useMemo(() => buildDailyTrend(daily, activity), [daily, activity]);
  const heatmap = useMemo(() => buildHourlyHeatmap(activity), [activity]);
  const sourceDist = useMemo(() => buildSourceDistribution(daily, sources), [daily, sources]);
  const modelDist = useMemo(() => buildModelDistribution(daily), [daily]);

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

  const DAY_LABELS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

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
                const label = m === 'token' ? 'Token' : m === 'cost' ? (locale === 'zh' ? '费用' : 'Cost') : (locale === 'zh' ? '时长' : 'Duration');
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
                  <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} />
                  <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8, fontSize: 11 }} />
                  <Bar dataKey="inputTokens" stackId="a" fill="var(--text-dim)" opacity={0.4} name="Input" />
                  <Bar dataKey="outputTokens" stackId="a" fill="var(--text)" name="Output" />
                  <Bar dataKey="cacheReadTokens" stackId="a" fill="var(--accent)" name="Cache Read" />
                </BarChart>
              ) : trendMetric === 'cost' ? (
                <BarChart data={trend}>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                  <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: number) => `$${v.toFixed(2)}`} />
                  <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8, fontSize: 11 }} formatter={(v: any) => [`$${Number(v).toFixed(4)}`, 'Cost']} />
                  <Bar dataKey="costUsd" fill="var(--success)" radius={[2, 2, 0, 0]} name="Cost" />
                </BarChart>
              ) : (
                <BarChart data={trend.map(t_ => ({ ...t_, activeMinutes: (t_.activeSeconds ?? 0) / 60 }))}>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis dataKey="date" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} tickFormatter={(v: string) => v.slice(5)} />
                  <YAxis tick={{ fontSize: 9, fill: 'var(--text-dim)' }} />
                  <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8, fontSize: 11 }} formatter={(v: any) => [`${Number(v).toFixed(1)} m`, 'Active Time']} />
                  <Bar dataKey="activeMinutes" fill="var(--accent)" radius={[2, 2, 0, 0]} name="Active Time" />
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
                const label = m === 'token' ? 'Token' : (locale === 'zh' ? '时长' : 'Duration');
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
                        const opacity = hasVal ? 0.15 + 0.85 * Math.sqrt(val / (grid.maxVal || 1)) : 0.05;
                        const tooltip = `${DAY_LABELS[rIdx]} ${cIdx.toString().padStart(2, '0')}:00 — ${
                          hasVal ? (heatmapMetric === 'token' ? val.toLocaleString() + ' tokens' : Math.round(val / 60) + ' min') : '0'
                        }`;
                        
                        const rowArr = grid.matrix[rIdx];
                        const valCheck = rowArr ? rowArr[cIdx] : 0;
                        const colorVar = valCheck > 0 ? 'var(--text)' : 'var(--text-dim)';

                        return (
                          <div
                            key={cIdx}
                            title={tooltip}
                            style={{
                              flex: 1,
                              aspectRatio: '1',
                              borderRadius: '2px',
                              background: colorVar,
                              opacity: opacity,
                              cursor: 'pointer',
                              transition: 'transform 0.1s ease',
                            }}
                            onMouseEnter={(e) => {
                              e.currentTarget.style.transform = 'scale(1.3)';
                              e.currentTarget.style.opacity = '1';
                              e.currentTarget.style.zIndex = '10';
                            }}
                            onMouseLeave={(e) => {
                              e.currentTarget.style.transform = 'none';
                              e.currentTarget.style.opacity = String(opacity);
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
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: SPACING.md }}>
        {/* Source Distribution */}
        <div style={{ padding: SPACING.md, borderRadius: BORDER_RADIUS.md, background: 'var(--bg-2)', border: '0.0625rem solid var(--border)' }}>
          <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-dim)', marginBottom: SPACING.sm }}>{t(locale, 'usage.sourceDistribution')}</div>
          {sourceDist.length > 0 ? (
            <ResponsiveContainer width="100%" height={180}>
              <BarChart data={sourceDist} layout="vertical">
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} />
                <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={60} />
                <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8 }} />
                <Bar dataKey="totalTokens" fill="var(--accent)" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')} />
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
                <XAxis type="number" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} />
                <YAxis type="category" dataKey="label" tick={{ fontSize: 9, fill: 'var(--text-dim)' }} width={80} />
                <Tooltip contentStyle={{ background: 'var(--bg)', border: '0.0625rem solid var(--border)', borderRadius: 8 }} />
                <Bar dataKey="totalTokens" fill="var(--accent)" radius={[0, 2, 2, 0]} name={t(locale, 'usage.totalTokens')} />
              </BarChart>
            </ResponsiveContainer>
          ) : (
            <EmptyState icon={<BarChart3 size={20} />} title={t(locale, 'usage.noModelData')} description="" />
          )}
        </div>
      </div>
    </div>
  );
}
