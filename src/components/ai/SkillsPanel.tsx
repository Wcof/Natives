'use client';

import { useState, useEffect, useCallback } from 'react';
import { type SkillInfo } from '@/types/agent';
import { Clipboard, RefreshCw } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState } from '@/components/ui/EmptyState';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

export default function SkillsPanel() {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [filter, setFilter] = useState<'all' | 'healthy' | 'issues'>('all');
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
        const result = await api.agent.scanSkills();
        if (Array.isArray(result)) {
          setSkills(result as SkillInfo[]);
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
        await api?.skills?.disable(skill.path);
      } else {
        await api?.skills?.enable(skill.path);
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
      await api?.skills?.uninstall(uninstallTarget.path);
      await loadSkills();
    } catch (err) {
      console.error('[SkillsPanel] Failed to uninstall skill:', err);
    } finally {
      setUninstallTarget(null);
    }
  }, [uninstallTarget, loadSkills]);

  useEffect(() => {
    loadSkills();
  }, [loadSkills]);

  const filtered = filter === 'all' ? skills : skills.filter((s) => filter === 'healthy' ? s.health.ok : !s.health.ok);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Filter tabs */}
      <div style={{
        display: 'flex', gap: SPACING.xs, padding: '8px 10px',
        borderBottom: '1px solid var(--vibe-btn-border)',
      }}>
        {(['all', 'healthy', 'issues'] as const).map((f) => (
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
            {f === 'all' ? t(locale, 'aiWorkbench.skillsLabel') : f === 'healthy' ? t(locale, 'aiWorkbench.skills.healthy') : t(locale, 'aiWorkbench.skills.issues')}
            {f !== 'all' && ` (${skills.filter((s) => f === 'healthy' ? s.health.ok : !s.health.ok).length})`}
          </button>
        ))}
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
          filtered.map((skill) => (
            <div key={skill.name} style={{
              padding: '8px 10px', marginBottom: SPACING.xs,
              borderRadius: BORDER_RADIUS.md,
              border: '1px solid var(--vibe-btn-border)',
              background: 'var(--vibe-toolbar-bg)',
            }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: SPACING.xs }}>
                <span style={{ fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text)' }}>{skill.name}</span>
                <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs }}>
                  <button onClick={() => handleToggle(skill)} style={{
                    fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
                    border: '1px solid var(--vibe-btn-border)',
                    background: skill.enabled ? 'var(--accent-soft)' : 'transparent',
                    color: skill.enabled ? 'var(--accent)' : 'var(--text-faint)',
                    cursor: 'pointer',
                  }}>
                    {skill.enabled ? t(locale, 'aiWorkbench.skills.enabled') : t(locale, 'aiWorkbench.skills.disabled')}
                  </button>
                  <span style={{ width: 8, height: 8, borderRadius: '50%', background: skill.health.ok ? 'var(--info)' : 'var(--danger)' }} />
                </div>
              </div>
              {skill.description && (
                <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', marginBottom: 2 }}>
                  {skill.description.slice(0, 100)}{skill.description.length > 100 ? '...' : ''}
                </div>
              )}
              <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)' }}>
                {t(locale, 'aiWorkbench.triggered')} {skill.triggerCount}x · {skill.source}
                {skill.lastTriggered ? <span> · last {new Date(skill.lastTriggered).toLocaleDateString()}</span> : null}
              </div>
              <div style={{ display: 'flex', gap: SPACING.xs, marginTop: 6 }}>
                <button onClick={() => { setSelectedSkill(skill); setShowLogs(true); const meta = [`Skill: ${skill.name}`, `Source: ${skill.source}`, `Path: ${skill.path}`, `Health: ${skill.health.ok ? 'OK' : skill.health.issues.join(', ') || 'unknown'}`, `Trigger count: ${skill.triggerCount}`, skill.lastTriggered ? `Last triggered: ${new Date(skill.lastTriggered).toLocaleString()}` : 'Last triggered: never']; if (skill.description) meta.push(`Description: ${skill.description.slice(0, 120)}`); setSkillLogs(meta); }} style={{ fontSize: 9, padding: '2px 5px', borderRadius: BORDER_RADIUS.sm, border: '1px solid var(--vibe-btn-border)', background: 'transparent', color: 'var(--text-faint)', cursor: 'pointer', display: 'inline-flex', alignItems: 'center', gap: 2 }}>
                  <Clipboard size={10} /> Logs
                </button>
                <button onClick={() => handleUninstall(skill)} style={{ fontSize: 9, padding: '2px 5px', borderRadius: BORDER_RADIUS.sm, border: '1px solid #f24b4b', background: 'transparent', color: 'var(--danger)', cursor: 'pointer' }}>
                  {t(locale, 'aiWorkbench.skills.uninstall')}
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      {/* Log viewer overlay (TASK-012) */}
      {showLogs && selectedSkill && (
        <div style={{
          position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.5)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 60,
        }} onClick={() => setShowLogs(false)}>
          <div style={{
            background: 'var(--vibe-toolbar-bg)', border: '1px solid var(--vibe-btn-border)',
            borderRadius: BORDER_RADIUS.xl, padding: 'var(--space-md)', width: 480, maxWidth: '90vw', maxHeight: '60vh',
            boxShadow: 'var(--vibe-toolbar-shadow)',
            backdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
            WebkitBackdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
            overflow: 'auto',
          }} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 'var(--fs-md)', fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.md, display: 'flex', alignItems: 'center', gap: SPACING.xs }}>
              <Clipboard size={14} /> {selectedSkill.name} — Skill Logs
            </div>
            <div style={{ fontSize: FONT_SIZE.sm, lineHeight: 1.6, fontFamily: 'var(--font-mono)' }}>
              {skillLogs.map((line, i) => (
                <div key={i} style={{ color: 'var(--text-dim)', whiteSpace: 'pre-wrap' }}>{line}</div>
              ))}
            </div>
          </div>
        </div>
      )}
      <ConfirmDialog
        open={!!uninstallTarget}
        title={t(locale,'aiWorkbench.skills.uninstall')}
        message={t(locale,'aiWorkbench.skills.confirmUninstall')}
        confirmLabel={t(locale,'aiWorkbench.skills.uninstall')}
        cancelLabel={t(locale,'common.cancel')}
        danger
        onConfirm={doUninstall}
        onCancel={() => setUninstallTarget(null)}
      />
    </div>
  );
}
