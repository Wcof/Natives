'use client';

import { startTransition, useState, useEffect, useCallback } from 'react';
import { type SkillInfo, type SkillsOverview } from '@/types/agent';
import { Clipboard, RefreshCw, AlertTriangle } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState } from '@/components/ui/EmptyState';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';
import Modal from '@/components/ui/Modal';

export default function SkillsPanel() {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [overview, setOverview] = useState<SkillsOverview | null>(null);
  const [filter, setFilter] = useState<'all' | 'healthy' | 'issues' | 'residue'>('all');
  const [loading, setLoading] = useState(true);
  const [locale, setLocale] = useState<Locale>('zh');
  const [selectedSkill, setSelectedSkill] = useState<SkillInfo | null>(null);
  const [showLogs, setShowLogs] = useState(false);
  const [skillLogs, setSkillLogs] = useState<string[]>([]);
  const [uninstallTarget, setUninstallTarget] = useState<SkillInfo | null>(null);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  const loadSkills = useCallback(async () => {
    setLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.agent?.scanSkills) {
        const result = await api.agent.scanSkills() as any;
        if (result && Array.isArray(result.items)) {
          setSkills(result.items as SkillInfo[]);
          setOverview(result.overview || null);
        } else if (Array.isArray(result)) {
          // 兼容旧格式
          setSkills(result as SkillInfo[]);
          setOverview(null);
        }
      }
    } catch (err) {
      console.error('[SkillsPanel] Failed to load skills:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  const handleToggle = useCallback(async (skill: SkillInfo) => {
    try {
      const api = window.nativesAPI;
      if (skill.enabled) {
        await api?.skills?.disable(skill.dir || skill.path);
      } else {
        await api?.skills?.enable(skill.dir || skill.path);
      }
      await loadSkills();
    } catch (err) {
      console.error('[SkillsPanel] Failed to toggle skill:', err);
    }
  }, [loadSkills]);

  const handleUninstall = useCallback((skill: SkillInfo) => {
    setUninstallTarget(skill);
  }, []);

  const doUninstall = useCallback(async () => {
    if (!uninstallTarget) return;
    try {
      const api = window.nativesAPI;
      await api?.skills?.uninstall(uninstallTarget.dir || uninstallTarget.path);
      await loadSkills();
    } catch (err) {
      console.error('[SkillsPanel] Failed to uninstall skill:', err);
    } finally {
      setUninstallTarget(null);
    }
  }, [uninstallTarget, loadSkills]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadSkills();
  }, [loadSkills]);

  const nonResidue = skills.filter((s) => !s.residue);
  const residueItems = skills.filter((s) => s.residue);

  const filtered = filter === 'all' ? skills
    : filter === 'healthy' ? nonResidue.filter((s) => s.health?.ok !== false)
    : filter === 'issues' ? nonResidue.filter((s) => s.health?.ok === false)
    : residueItems;

  const budgetPercent = overview
    ? Math.min(100, Math.round((overview.budgetChars / overview.budgetLimit) * 100))
    : 0;
  const budgetOver = overview ? overview.budgetChars > overview.budgetLimit : false;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* 概览统计栏 */}
      {overview && (
        <div style={{
          display: 'flex', gap: SPACING.md, padding: '8px 10px', flexWrap: 'wrap',
          borderBottom: '1px solid var(--vibe-btn-border)',
          fontSize: FONT_SIZE.xs, color: 'var(--text-dim)',
        }}>
          <span>{t(locale, 'aiWorkbench.skills.total')} <b style={{ color: 'var(--text)' }}>{overview.total}</b></span>
          <span>{t(locale, 'aiWorkbench.skillsLabel')} <b style={{ color: 'var(--text)' }}>{overview.unique}</b></span>
          <span>{t(locale, 'aiWorkbench.skills.active')} <b style={{ color: 'var(--info)' }}>{overview.active}</b></span>
          <span>{t(locale, 'aiWorkbench.skills.dust')} <b style={{ color: 'var(--text-faint)' }}>{overview.dust}</b></span>
          <span>{t(locale, 'aiWorkbench.skills.issues')} <b style={{ color: overview.issues > 0 ? 'var(--danger)' : 'var(--text-faint)' }}>{overview.issues}</b></span>
          {residueItems.length > 0 && (
            <span style={{ color: 'var(--warning)' }}>⚠ {residueItems.length} residue</span>
          )}
        </div>
      )}

      {/* 描述预算条 */}
      {overview && (
        <div style={{ padding: '4px 10px 6px', borderBottom: '1px solid var(--vibe-btn-border)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 9, color: 'var(--text-dim)', marginBottom: 2 }}>
            <span>{t(locale, 'aiWorkbench.skills.budget')}</span>
            <span style={{ color: budgetOver ? 'var(--danger)' : 'var(--text-dim)' }}>
              {overview.budgetChars.toLocaleString()} / {overview.budgetLimit.toLocaleString()}
            </span>
          </div>
          <div style={{
            height: 3, borderRadius: 2, background: 'var(--vibe-btn-border)',
            overflow: 'hidden',
          }}>
            <div style={{
              height: '100%', width: `${budgetPercent}%`,
              background: budgetOver ? 'var(--danger)' : budgetPercent > 80 ? 'var(--warning)' : 'var(--info)',
              transition: 'width 0.3s',
            }} />
          </div>
          {budgetOver && (
            <div style={{ fontSize: 9, color: 'var(--danger)', marginTop: 2 }}>
              {t(locale, 'aiWorkbench.skills.budgetWarning')}
            </div>
          )}
        </div>
      )}

      {/* Filter tabs */}
      <div style={{
        display: 'flex', gap: SPACING.xs, padding: '6px 10px',
        borderBottom: '1px solid var(--vibe-btn-border)',
      }}>
        {(['all', 'healthy', 'issues', 'residue'] as const).map((f) => {
          const count = f === 'all' ? skills.length
            : f === 'healthy' ? nonResidue.filter((s) => s.health.ok).length
            : f === 'issues' ? nonResidue.filter((s) => !s.health.ok).length
            : residueItems.length;
          return (
            <button
              key={f}
              className="btn-ghost"
              onClick={() => setFilter(f)}
              style={{
                fontSize: FONT_SIZE.xs, padding: '3px 8px', borderRadius: BORDER_RADIUS.sm,
                color: filter === f ? 'var(--accent)' : 'var(--text-faint)',
                background: filter === f ? 'var(--accent-soft)' : 'transparent',
              }}
            >
              {f === 'all' ? t(locale, 'aiWorkbench.skillsLabel')
                : f === 'healthy' ? t(locale, 'aiWorkbench.skills.healthy')
                : f === 'issues' ? t(locale, 'aiWorkbench.skills.issues')
                : 'Residue'}
              {' '}({count})
            </button>
          );
        })}
        <div style={{ flex: 1 }} />
        <button className="btn-ghost" onClick={loadSkills} style={{ fontSize: FONT_SIZE.xs, padding: '3px 6px' }}>
          <RefreshCw size={12} />
        </button>
      </div>

      {/* Skills list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {loading ? (
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
            {t(locale, 'common.loading')}
          </div>
        ) : filtered.length === 0 ? (
          <EmptyState title={t(locale, 'aiWorkbench.noSkills')} />
        ) : (
          filtered.map((skill, index) => (
            <div key={`${skill.source}:${skill.name}-${index}`} style={{
              padding: '8px 10px', marginBottom: SPACING.xs,
              borderRadius: BORDER_RADIUS.md,
              border: `1px solid ${skill.residue ? 'var(--warning)' : 'var(--vibe-btn-border)'}`,
              background: skill.residue ? 'rgba(255,180,0,0.05)' : 'var(--vibe-toolbar-bg)',
              opacity: skill.enabled ? 1 : 0.6,
            }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: SPACING.xs }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs }}>
                  <span style={{ fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text)' }}>{skill.name}</span>
                  {skill.residue && (
                    <span style={{
                      fontSize: 8, padding: '1px 4px', borderRadius: 3,
                      background: 'var(--warning)', color: '#000', fontWeight: 600,
                    }}>RESIDUE</span>
                  )}
                  {skill.copies && skill.copies.length > 1 && (
                    <span style={{
                      fontSize: 8, padding: '1px 4px', borderRadius: 3,
                      border: '1px solid var(--info)', color: 'var(--info)',
                    }}>×{skill.copies.length}</span>
                  )}
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs }}>
                  {!skill.residue && (
                    <button onClick={() => handleToggle(skill)} style={{
                      fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
                      border: '1px solid var(--vibe-btn-border)',
                      background: skill.enabled ? 'var(--accent-soft)' : 'transparent',
                      color: skill.enabled ? 'var(--accent)' : 'var(--text-faint)',
                      cursor: 'pointer',
                    }}>
                      {skill.enabled ? t(locale, 'aiWorkbench.skills.enabled') : t(locale, 'aiWorkbench.skills.disabled')}
                    </button>
                  )}
                  <span style={{
                    width: 8, height: 8, borderRadius: '50%',
                    background: skill.residue ? 'var(--warning)' : skill.health?.ok !== false ? 'var(--info)' : 'var(--danger)',
                  }} />
                </div>
              </div>

              {/* Description */}
              {skill.description && (
                <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', marginBottom: 2 }}>
                  {skill.description.slice(0, 100)}{skill.description.length > 100 ? '...' : ''}
                </div>
              )}

              {/* 健康问题详情 */}
              {skill.health?.ok === false && (skill.health?.issues?.length ?? 0) > 0 && (
                <div style={{ fontSize: 9, color: 'var(--danger)', marginBottom: 2, display: 'flex', alignItems: 'center', gap: 3 }}>
                  <AlertTriangle size={10} />
                  {skill.health.issues[0]}
                </div>
              )}

              {/* 跨来源副本 */}
              {skill.copies && skill.copies.length > 1 && (
                <div style={{ fontSize: 9, color: 'var(--info)', marginBottom: 2 }}>
                  {skill.copies.join(' · ')}
                </div>
              )}

              {/* Meta info */}
              <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)' }}>
                {t(locale, 'aiWorkbench.triggered')} {skill.triggerCount || 0}x · {skill.label || skill.source}
                {skill.lastTriggered ? <span> · {new Date(skill.lastTriggered).toLocaleDateString()}</span> : null}
                {skill.descLen && skill.descLen > (overview?.descCut || 1536) ? (
                  <span style={{ color: 'var(--warning)' }}> · desc {skill.descLen} chars</span>
                ) : null}
              </div>

              {/* Actions */}
              <div style={{ display: 'flex', gap: SPACING.xs, marginTop: 6 }}>
                <button onClick={() => {
                  setSelectedSkill(skill);
                  setShowLogs(true);
                  const meta = [
                    `Skill: ${skill.name}`,
                    `Source: ${skill.source} (${skill.label || ''})`,
                    `Dir: ${skill.dir || skill.path}`,
                    `Enabled: ${skill.enabled}`,
                    `Residue: ${!!skill.residue}`,
                    `Health: ${skill.health?.ok !== false ? 'OK' : (skill.health?.issues ?? []).join(', ') || 'unknown'}`,
                    `Trigger count: ${skill.triggerCount || 0}`,
                    skill.lastTriggered ? `Last triggered: ${new Date(skill.lastTriggered).toLocaleString()}` : 'Last triggered: never',
                    skill.descLen ? `Description length: ${skill.descLen}` : '',
                    skill.copies ? `Copies: ${skill.copies.join(', ')}` : '',
                  ].filter(Boolean);
                  if (skill.description) meta.push(`Description: ${skill.description.slice(0, 200)}`);
                  setSkillLogs(meta);
                }} style={{
                  fontSize: 9, padding: '2px 5px', borderRadius: BORDER_RADIUS.sm,
                  border: '1px solid var(--vibe-btn-border)', background: 'transparent',
                  color: 'var(--text-faint)', cursor: 'pointer',
                  display: 'inline-flex', alignItems: 'center', gap: 2,
                }}>
                  <Clipboard size={10} /> Logs
                </button>
                <button onClick={() => handleUninstall(skill)} style={{
                  fontSize: 9, padding: '2px 5px', borderRadius: BORDER_RADIUS.sm,
                  border: '1px solid var(--danger)', background: 'transparent',
                  color: 'var(--danger)', cursor: 'pointer',
                }}>
                  {t(locale, 'aiWorkbench.skills.uninstall')}
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      {/* Log viewer overlay */}
      <Modal
        isOpen={showLogs && !!selectedSkill}
        onClose={() => setShowLogs(false)}
        title={selectedSkill ? `${selectedSkill.name} — Skill Logs` : 'Skill Logs'}
        width={480}
      >
        <div style={{ fontSize: FONT_SIZE.sm, lineHeight: 1.6, fontFamily: 'var(--font-mono)', maxHeight: '50vh', overflowY: 'auto' }}>
          {skillLogs.map((line, i) => (
            <div key={i} style={{ color: 'var(--text-dim)', whiteSpace: 'pre-wrap' }}>{line}</div>
          ))}
        </div>
      </Modal>
      <ConfirmDialog
        open={!!uninstallTarget}
        title={t(locale, 'aiWorkbench.skills.uninstall')}
        message={t(locale, 'aiWorkbench.skills.confirmUninstall')}
        confirmLabel={t(locale, 'aiWorkbench.skills.uninstall')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={doUninstall}
        onCancel={() => setUninstallTarget(null)}
      />
    </div>
  );
}
