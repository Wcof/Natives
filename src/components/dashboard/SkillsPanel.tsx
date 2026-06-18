'use client';

import { useEffect, useState } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useTranslation } from 'react-i18next';
import { Loader2, Code2, Zap, Filter, ChevronDown, Trophy, Calendar } from 'lucide-react';
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

export function SkillsPanel({ skills, isLoading }: SkillsPanelProps) {
  const { t } = useTranslation();
  const [skillUsages, setSkillUsages] = useState<SkillUsage[]>([]);
  const [sortBy, setSortBy] = useState<'hits' | 'recent'>('hits');
  const [isSortDropdownOpen, setIsSortDropdownOpen] = useState(false);

  useEffect(() => {
    if (skills) {
      // In production, hitCount and lastTriggered would come from a usage tracking API
      // For now, we display the skill list with available info
      const usages: SkillUsage[] = skills.map(skill => ({
        skill,
        hitCount: skill.hitCount ?? 0,
        lastTriggered: skill.lastTriggered,
      }));
      setSkillUsages(usages);
    } else {
      setSkillUsages([]);
    }
  }, [skills]);

  const sortedUsages = [...skillUsages].sort((a, b) => {
    if (sortBy === 'hits') {
      return b.hitCount - a.hitCount;
    }
    // Sort by recent
    if (!a.lastTriggered && !b.lastTriggered) return 0;
    if (!a.lastTriggered) return 1;
    if (!b.lastTriggered) return -1;
    return b.lastTriggered - a.lastTriggered;
  });

  const enabledCount = skillUsages.filter(s => s.skill.enabled).length;
  const sourceCounts = skillUsages.reduce((acc, s) => {
    const src = s.skill.source ?? 'installed';
    acc[src] = (acc[src] ?? 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  const sourceDistribution = Object.entries(sourceCounts)
    .map(([src, count]) => `${getSourceLabel(src)}: ${count}`)
    .join('  ·  ');

  if (isLoading) {
    return (
      <div
        style={{
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
        <Loader2 size={24} style={{ color: 'var(--text-faint)', animation: 'spin 1s linear infinite' }} />
      </div>
    );
  }

  return (
    <div
      style={{
        borderRadius: BORDER_RADIUS.lg,
        border: '0.0625rem solid var(--vibe-content-border)',
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
        padding: `${SPACING.lg}px`,
      }}
    >
      {/* Header */}
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
            {t('dashboard.skillsTitle')}
          </h3>
          <p style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)' }}>
            {t('dashboard.installed')}: {skillUsages.length}  ·  {t('dashboard.enabled')}: {enabledCount}
            {sourceDistribution && `  ·  ${t('dashboard.sourceDistribution')}: ${sourceDistribution}`}
          </p>
        </div>
      </div>

      {/* Sort dropdown */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: SPACING.sm }}>
        <h4 style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
          {t('dashboard.usageRank')}
        </h4>
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
            <span>{sortBy === 'hits' ? t('dashboard.hitCount') : t('dashboard.lastTriggered')}</span>
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
                background: 'var(--vibe-content-bg)',
                border: '0.0625rem solid var(--vibe-content-border)',
                boxShadow: '0 8px 24px rgba(0,0,0,0.4)',
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
                  color: sortBy === 'hits' ? 'var(--vibe-active-color)' : 'var(--vibe-brand-text)',
                  background: sortBy === 'hits' ? 'var(--vibe-active-bg)' : 'transparent',
                  cursor: 'pointer',
                }}
              >
                {t('dashboard.hitCount')}
              </button>
              <button
                onClick={() => { setSortBy('recent'); setIsSortDropdownOpen(false); }}
                style={{
                  display: 'block',
                  width: '100%',
                  padding: `${SPACING.xs}px ${SPACING.sm}px`,
                  textAlign: 'left',
                  fontSize: FONT_SIZE.xs,
                  color: sortBy === 'recent' ? 'var(--vibe-active-color)' : 'var(--vibe-brand-text)',
                  background: sortBy === 'recent' ? 'var(--vibe-active-bg)' : 'transparent',
                  cursor: 'pointer',
                  borderTop: '0.0625rem solid var(--vibe-content-border)',
                }}
              >
                {t('dashboard.lastTriggered')}
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Skills list */}
      {sortedUsages.length === 0 ? (
        <div style={{ textAlign: 'center', padding: `${SPACING.xl}px 0`, color: 'var(--text-faint)', fontSize: FONT_SIZE.sm }}>
          {t('dashboard.noData')}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 0 }}>
          {sortedUsages.map((usage, index) => (
            <div
              key={usage.skill.id ?? usage.skill.name}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: SPACING.sm,
                padding: `${SPACING.sm}px ${SPACING.md}px`,
                borderRadius: index === 0 ? BORDER_RADIUS.md : 0,
                background: index === 0 ? 'var(--vibe-active-bg)' : 'transparent',
                borderTop: '0.0625rem solid var(--vibe-content-border)',
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

              {/* Name */}
              <div style={{ flex: 1, minWidth: 0 }}>
                <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 500, color: 'var(--vibe-brand-text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {usage.skill.name}
                </span>
                {usage.skill.enabled && (
                  <span style={{
                    display: 'inline-block',
                    marginLeft: SPACING.xs,
                    padding: '1px 6px',
                    borderRadius: '999px',
                    fontSize: '9px',
                    color: 'var(--vibe-active-color)',
                    background: 'var(--vibe-active-bg)',
                  }}>
                    {t('dashboard.enabled')}
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
          ))}
        </div>
      )}
    </div>
  );
}
