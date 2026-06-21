'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import ShortcutHelp from '@/components/ui/ShortcutHelp';

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: SPACING.xs / 2 }}>{label}</div>
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

  useEffect(() => {
    async function load() {
      try {
        const api = window.nativesAPI;
        if (!api?.module?.list) return;
        const list = await api.module.list();
        if (Array.isArray(list)) {
          const found = (list as Array<{ id: string; name: string; version: string; enabled: number; state: string; description?: string; author?: string }>).find((m) => m.id === moduleId);
          if (found) setMod(found);
        }
        if (api?.module?.listPermissions) {
          const perms = await api.module.listPermissions(moduleId);
          if (Array.isArray(perms)) setModulePerms(perms as unknown as Array<{ module_id: string; permission: string; granted: number }>);
        }
      } catch { /* ignore */ }
    }
    load();
  }, [moduleId]);

  if (!mod) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', padding: SPACING.xl, minHeight: 120 }}>
        <MathCurveLoader size={36} />
      </div>
    );
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
          <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: SPACING.xs }}>
            Permissions
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.xs }}>
            {modulePerms.map((perm) => (
              <div key={perm.permission} style={{
                display: 'flex', alignItems: 'center', gap: SPACING.sm,
                padding: `${SPACING.xs}px ${SPACING.sm}px`, background: 'var(--vibe-btn-bg)',
                borderRadius: BORDER_RADIUS.sm, fontSize: FONT_SIZE.sm,
              }}>
                <span style={{ color: perm.granted ? 'var(--accent)' : 'var(--text-faint)' }}>
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
