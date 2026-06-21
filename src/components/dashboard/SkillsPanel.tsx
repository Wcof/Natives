'use client';

import { useState, useMemo } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS, SHADOW } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import { Code2, Zap, Filter, ChevronDown, Trophy, Calendar } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import type { SkillInfo } from '@/types/agent';

/**
 * SkillsPanel — Skills usage and insights panel
 *
 * Layout:
 * ┌─────────────────────────────────────────────────────┐
 * │  Skills 总览                                         │
 * │  已安装: 12   已启用: 8   来源分布: [global:3]      │
 * ├─────────────────────────────────────────────────────┤
 * │  使用频次排行榜                                      │
 * │  #1 code-review     156次   最近: 2小时前           │
 * │  #2 file-organize   89次    最近: 昨天              │
 * └─────────────────────────────────────────────────────┘
 *
 * Data source: window.nativesAPI.agent.scanSkills() (existing API)
 * Design: Follows vibe-* glassmorphic tokens
 */

interface SkillsPanelProps {
  skills?: SkillInfo[];
  isLoading?: boolean;
  minimal?: boolean;
}

interface SkillUsage {
  skill: SkillInfo;
  hitCount: number;
  lastTriggered?: number; // epoch ms
}

function formatTimeAgo(epochMs?: number): string {
  if (!epochMs) return 'never';
  const now = Date.now();
  const diff = now - epochMs;
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(diff / 3600000);
  const days = Math.floor(diff / 86400000);

  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  if (hours < 24) return `${hours}h ago`;
  if (days < 7) return `${days}d ago`;
  return new Date(epochMs).toLocaleDateString();
}

function getSourceLabel(source: string) {
  const map: Record<string, string> = {
    global: 'Global',
    project: 'Project',
    plugin: 'Plugin',
    installed: 'Installed',
  };
  return map[source] || source;
}

function getToolContext(source: string): { label: string; bg: string; color: string } | null {
  const src = source.toLowerCase();
  if (src.includes('claude') || src.includes('claude-plugins')) {
    return { label: 'Claude', bg: '#3b82f61a', color: '#3b82f6' };
  }
  if (src.includes('codex')) {
    return { label: 'Codex', bg: '#8b5cf61a', color: '#8b5cf6' };
  }
  if (src.includes('agents')) {
    return { label: 'Natives', bg: '#10b9811a', color: '#10b981' };
  }
  return null;
}

export function SkillsPanel({ skills, isLoading, minimal }: SkillsPanelProps) {
  const locale = useLocale();
  const [sortBy, setSortBy] = useState<'hits' | 'recent'>('hits');
  const [isSortDropdownOpen, setIsSortDropdownOpen] = useState(false);
  const [toolFilter, setToolFilter] = useState<'all' | 'claude' | 'codex'>('all');

  const skillUsages: SkillUsage[] = useMemo(() => {
    if (!skills) return [];
    return skills.map(skill => ({
      skill,
      hitCount: skill.hitCount ?? 0,
      lastTriggered: skill.lastTriggered,
    }));
  }, [skills]);

  const sortedUsages = useMemo(() => {
    return [...skillUsages].sort((a, b) => {
      if (sortBy === 'hits') {
        return b.hitCount - a.hitCount;
      }
      if (!a.lastTriggered && !b.lastTriggered) return 0;
      if (!a.lastTriggered) return 1;
      if (!b.lastTriggered) return -1;
      return b.lastTriggered - a.lastTriggered;
    });
  }, [skillUsages, sortBy]);

  const filteredUsages = useMemo(() => {
    return sortedUsages.filter(usage => {
      const src = (usage.skill.source || '').toLowerCase();
      if (toolFilter === 'all') return true;
      if (toolFilter === 'claude') return src.includes('claude') || src.includes('claude-plugins');
      if (toolFilter === 'codex') return src.includes('codex');
      return true;
    });
  }, [sortedUsages, toolFilter]);

  const enabledCount = useMemo(() => skillUsages.filter(s => s.skill.enabled).length, [skillUsages]);

  const sourceDistribution = useMemo(() => {
    const sourceCounts = skillUsages.reduce((acc, s) => {
      const src = s.skill.source ?? 'installed';
      acc[src] = (acc[src] ?? 0) + 1;
      return acc;
    }, {} as Record<string, number>);
    return Object.entries(sourceCounts)
      .map(([src, count]) => `${getSourceLabel(src)}: ${count}`)
      .join('  ·  ');
  }, [skillUsages]);

  if (isLoading) {
    return (
      <div
        style={minimal ? {
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          minHeight: 120,
        } : {
          borderRadius: BORDER_RADIUS.lg,
          border: '0.0625rem solid var(--vibe-content-border)',
          background: 'var(--vibe-content-bg)',
          backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
          padding: `${SPACING.lg}px`,
          minHeight: 200,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <MathCurveLoader size={36} />
      </div>
    );
  }

  return (
    <div
      style={minimal ? {
        paddingTop: SPACING.md,
      } : {
        borderRadius: BORDER_RADIUS.lg,
        border: '0.0625rem solid var(--vibe-content-border)',
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
        padding: `${SPACING.lg}px`,
      }}
    >
      {/* Header — hide in minimal mode since KanbanCard already displays header */}
      {!minimal && (
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.md }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: 32,
              height: 32,
              borderRadius: BORDER_RADIUS.md,
              background: 'var(--vibe-accent-soft)',
              color: 'var(--vibe-accent-color)',
            }}
          >
            <Code2 size={16} />
          </div>
          <div>
            <h3 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>
              {t(locale, 'dashboard.skillsTitle')}
            </h3>
            <p style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)' }}>
              {t(locale, 'dashboard.installed')}: {skillUsages.length}  ·  {t(locale, 'dashboard.enabled')}: {enabledCount}
              {sourceDistribution && `  ·  ${t(locale, 'dashboard.sourceDistribution')}: ${sourceDistribution}`}
            </p>
          </div>
        </div>
      )}

      {/* Sort dropdown & Tool Filter tabs */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: SPACING.sm, gap: SPACING.sm, flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.md }}>
          <h4 style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em', margin: 0 }}>
            {t(locale, 'dashboard.usageRank')}
          </h4>
          <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, background: 'var(--vibe-btn-bg)', padding: SPACING.xs, borderRadius: BORDER_RADIUS.md, border: '0.0625rem solid var(--vibe-btn-border)' }}>
            {(['all', 'claude', 'codex'] as const).map(filterVal => {
              const active = toolFilter === filterVal;
              return (
                <button
                  key={filterVal}
                  onClick={() => setToolFilter(filterVal)}
                  style={{
                    padding: `${SPACING.xs}px ${SPACING.sm}px`,
                    borderRadius: BORDER_RADIUS.sm,
                    fontSize: FONT_SIZE.xs,
                    fontWeight: active ? 600 : 400,
                    background: active ? 'var(--vibe-active-bg)' : 'transparent',
                    color: active ? 'var(--vibe-active-color)' : 'var(--text-dim)',
                    border: 'none',
                    cursor: 'pointer',
                    transition: 'all 0.15s ease',
                  }}
                >
                  {filterVal === 'all' ? 'All' : filterVal === 'claude' ? 'Claude' : 'Codex'}
                </button>
              );
            })}
          </div>
        </div>
        <div style={{ position: 'relative' }}>
          <button
            onClick={() => setIsSortDropdownOpen(!isSortDropdownOpen)}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: SPACING.xs,
              padding: `${SPACING.xs}px ${SPACING.sm}px`,
              borderRadius: BORDER_RADIUS.sm,
              background: 'var(--vibe-btn-bg)',
              border: '0.0625rem solid var(--vibe-btn-border)',
              fontSize: FONT_SIZE.xs,
              color: 'var(--vibe-brand-text)',
              cursor: 'pointer',
            }}
          >
            <Filter size={12} />
            <span>{sortBy === 'hits' ? t(locale, 'dashboard.hitCount') : t(locale, 'dashboard.lastTriggered')}</span>
            <ChevronDown size={12} />
          </button>
          {isSortDropdownOpen && (
            <div
              style={{
                position: 'absolute',
                right: 0,
                top: '100%',
                marginTop: SPACING.xs,
                borderRadius: BORDER_RADIUS.sm,
                background: 'var(--bg-3)',
                border: '0.0625rem solid var(--border)',
                boxShadow: SHADOW.elevated,
                zIndex: 100,
                minWidth: 140,
              }}
            >
              <button
                onClick={() => { setSortBy('hits'); setIsSortDropdownOpen(false); }}
                style={{
                  display: 'block',
                  width: '100%',
                  padding: `${SPACING.xs}px ${SPACING.sm}px`,
                  textAlign: 'left',
                  fontSize: FONT_SIZE.xs,
                  color: sortBy === 'hits' ? 'var(--vibe-active-color)' : 'var(--text)',
                  background: sortBy === 'hits' ? 'var(--vibe-active-bg)' : 'transparent',
                  cursor: 'pointer',
                  border: 'none',
                }}
              >
                {t(locale, 'dashboard.hitCount')}
              </button>
              <button
                onClick={() => { setSortBy('recent'); setIsSortDropdownOpen(false); }}
                style={{
                  display: 'block',
                  width: '100%',
                  padding: `${SPACING.xs}px ${SPACING.sm}px`,
                  textAlign: 'left',
                  fontSize: FONT_SIZE.xs,
                  color: sortBy === 'recent' ? 'var(--vibe-active-color)' : 'var(--text)',
                  background: sortBy === 'recent' ? 'var(--vibe-active-bg)' : 'transparent',
                  cursor: 'pointer',
                  border: 'none',
                  borderTop: '0.0625rem solid var(--border)',
                }}
              >
                {t(locale, 'dashboard.lastTriggered')}
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Skills list */}
      {filteredUsages.length === 0 ? (
        <div style={{ textAlign: 'center', padding: `${SPACING.xl}px 0`, color: 'var(--text-faint)', fontSize: FONT_SIZE.sm }}>
          {t(locale, 'dashboard.noData')}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 0 }}>
          {filteredUsages.map((usage, index) => {
            const toolContext = getToolContext(usage.skill.source || '');
            return (
              <div
                key={`${usage.skill.source ?? 'unknown'}:${usage.skill.id ?? usage.skill.name}-${index}`}
                onClick={() => {
                  if (usage.skill.path) {
                    window.dispatchEvent(new CustomEvent('navigate', {
                      detail: `files?path=${encodeURIComponent(usage.skill.path)}`
                    }));
                  }
                }}
                title={usage.skill.path ? `${t(locale, 'dashboard.clickToNavigate') || '点击跳转'}: ${usage.skill.path}` : undefined}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: SPACING.sm,
                  padding: `${SPACING.sm}px ${SPACING.md}px`,
                  borderRadius: index === 0 ? BORDER_RADIUS.md : 0,
                  background: index === 0 ? 'var(--vibe-active-bg)' : 'transparent',
                  borderTop: '0.0625rem solid var(--vibe-content-border)',
                  cursor: usage.skill.path ? 'pointer' : 'default',
                  transition: 'background 0.2s ease, transform 0.2s ease',
                }}
                onMouseEnter={(e) => {
                  if (usage.skill.path) {
                    (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)';
                    (e.currentTarget as HTMLElement).style.transform = 'translateX(3px)';
                  }
                }}
                onMouseLeave={(e) => {
                  (e.currentTarget as HTMLElement).style.background = index === 0 ? 'var(--vibe-active-bg)' : 'transparent';
                  (e.currentTarget as HTMLElement).style.transform = 'none';
                }}
              >
                {/* Rank */}
                <div style={{ width: 24, textAlign: 'center' }}>
                  {index === 0 ? (
                    <Trophy size={16} style={{ color: '#fbbf24' }} />
                  ) : (
                    <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)' }}>
                      #{index + 1}
                    </span>
                  )}
                </div>

                {/* Icon */}
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    width: 28,
                    height: 28,
                    borderRadius: BORDER_RADIUS.sm,
                    background: 'var(--vibe-btn-bg)',
                  }}
                >
                  {usage.skill.icon ? (
                    <img src={usage.skill.icon} alt="" style={{ width: 16, height: 16 }} />
                  ) : (
                    <Zap size={14} style={{ color: 'var(--vibe-accent-color)' }} />
                  )}
                </div>

                {/* Name & Context Badges */}
                <div style={{ flex: 1, minWidth: 0, display: 'flex', alignItems: 'center', gap: SPACING.xs, flexWrap: 'wrap' }}>
                  <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 500, color: 'var(--vibe-brand-text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {usage.skill.name}
                  </span>
                  {toolContext && (
                    <span style={{
                      display: 'inline-block',
                      padding: `1px ${SPACING.xs}px`,
                      borderRadius: '999px',
                      fontSize: FONT_SIZE.xs,
                      color: toolContext.color,
                      background: toolContext.bg,
                      fontWeight: 600,
                    }}>
                      {toolContext.label}
                    </span>
                  )}
                  {usage.skill.enabled && (
                    <span style={{
                      display: 'inline-block',
                      padding: `1px ${SPACING.xs}px`,
                      borderRadius: '999px',
                      fontSize: FONT_SIZE.xs,
                      color: 'var(--vibe-active-color)',
                      background: 'var(--vibe-active-bg)',
                    }}>
                      {t(locale, 'dashboard.enabled')}
                    </span>
                  )}
                </div>

                {/* Hit count */}
                <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, minWidth: 70 }}>
                  <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)', fontFamily: 'var(--font-mono)' }}>
                    {usage.hitCount.toLocaleString()}
                  </span>
                  <span style={{ fontSize: '10px', color: 'var(--text-faint)' }}>hits</span>
                </div>

                {/* Last triggered */}
                <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, minWidth: 80 }}>
                  <Calendar size={12} style={{ color: 'var(--text-faint)' }} />
                  <span style={{ fontSize: '10px', color: 'var(--text-dim)' }}>
                    {formatTimeAgo(usage.lastTriggered)}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
