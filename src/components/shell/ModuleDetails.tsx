'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { ErrorState, EmptyState } from '@/components/ui/EmptyState';
import { classifyError } from '@/lib/error-classifier';
import ShortcutHelp from '@/components/ui/ShortcutHelp';

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: SPACING.xs / 2 }}>{label}</div>
      <div style={{ color: 'var(--text)', wordBreak: 'break-all' }}>{value}</div>
    </div>
  );
}

interface ModuleDetailsProps {
  moduleId: string;
  locale: Locale;
}

export default function ModuleDetails({ moduleId, locale }: ModuleDetailsProps) {
  const [mod, setMod] = useState<{ name: string; version: string; enabled: number; state: string; description?: string; author?: string } | null>(null);
  const [modulePerms, setModulePerms] = useState<Array<{ module_id: string; permission: string; granted: number }>>([]);
  // 三态：旧实现失败/未找到时永久转圈（catch{} + !mod → loader）
  const [status, setStatus] = useState<'loading' | 'error' | 'missing' | 'ready'>('loading');
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      setStatus('loading');
      try {
        const api = window.nativesAPI;
        if (!api?.module?.list) throw new Error('module API unavailable');
        const list = await api.module.list();
        if (cancelled) return;
        const found = Array.isArray(list)
          ? (list as Array<{ id: string; name: string; version: string; enabled: number; state: string; description?: string; author?: string }>).find((m) => m.id === moduleId)
          : undefined;
        if (!found) { setStatus('missing'); return; }
        setMod(found);
        if (api?.module?.listPermissions) {
          const perms = await api.module.listPermissions(moduleId);
          if (!cancelled && Array.isArray(perms)) setModulePerms(perms as unknown as Array<{ module_id: string; permission: string; granted: number }>);
        }
        if (!cancelled) setStatus('ready');
      } catch (e) {
        if (!cancelled) {
          setErrorMsg(classifyError(e).userMessage);
          setStatus('error');
        }
      }
    }
    void load();
    return () => { cancelled = true; };
  }, [moduleId]);

  if (status === 'loading') {
    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', padding: SPACING.xl, minHeight: 120 }}>
        <MathCurveLoader size={36} />
      </div>
    );
  }
  if (status === 'error') {
    return <ErrorState message={errorMsg ?? ''} />;
  }
  if (status === 'missing' || !mod) {
    return <EmptyState title={t(locale, 'errors.moduleNotFoundNamed', { id: moduleId })} />;
  }

  return (
    <div style={{ padding: SPACING.lg, fontSize: FONT_SIZE.md }}>
      <div style={{ fontSize: FONT_SIZE.xl, fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.lg }}>{mod.name}</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
        <InfoRow label={t(locale, 'workshop.templateId')} value={moduleId} />
        <InfoRow label={t(locale, 'store.installed')} value={t(locale, mod.enabled ? 'workshop.enabled' : 'workshop.disabled')} />
        {mod.version && <InfoRow label={t(locale, 'store.version')} value={'v' + mod.version} />}
        {mod.author && <InfoRow label={t(locale, 'store.author')} value={mod.author} />}
        {mod.description && <InfoRow label={t(locale, 'store.description')} value={mod.description} />}
      </div>
      {modulePerms.length > 0 && (
        <div style={{ marginTop: SPACING.lg }}>
          <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: SPACING.xs }}>
            {t(locale, 'workshop.permissionsTitle')}
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.xs }}>
            {modulePerms.map((perm) => (
              <div key={perm.permission} style={{
                display: 'flex', alignItems: 'center', gap: SPACING.sm,
                padding: `${SPACING.xs}px ${SPACING.sm}px`, background: 'var(--surface)',
                borderRadius: BORDER_RADIUS.sm, fontSize: FONT_SIZE.sm,
              }}>
                <span style={{ color: perm.granted ? 'var(--primary)' : 'var(--text-disabled)' }}>
                  {perm.granted ? '✓' : '✗'}
                </span>
                <span style={{ color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>{perm.permission}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      <ShortcutHelp />
    </div>
  );
}
