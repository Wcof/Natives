'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { t, useLocale } from '@/i18n';
import { createDefaultGateway } from '@/lib/assistant-gateway';
import type { DaemonCapabilities } from '@/lib/assistant-protocol';
import { canManageCapabilities } from '@/lib/assistant-workspace/capability-gate';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import { Blocks } from 'lucide-react';
import SkillsTab from './skills/SkillsTab';
import ConnectorsTab from './connectors/ConnectorsTab';
import ExpertsTab from './experts/ExpertsTab';

type CapabilitiesTab = 'skills' | 'connectors' | 'experts';

/**
 * 能力库（Capability Hub, ADR-0016）三 Tab 壳：Skills / 连接器 / 专家。
 * Honest gating: nothing is rendered unless the daemon advertises capability.*.
 */
export default function CapabilitiesPage() {
  const locale = useLocale();
  const [tab, setTab] = useState<CapabilitiesTab>('skills');
  const [caps, setCaps] = useState<DaemonCapabilities | null>(null);
  const [phase, setPhase] = useState<'loading' | 'error' | 'ready'>('loading');
  const [error, setError] = useState<string | null>(null);

  const gateway = useMemo(() => createDefaultGateway(false), []);

  const loadCapabilities = useCallback(async () => {
    setPhase('loading');
    setError(null);
    try {
      await gateway.connect();
      const capabilities = gateway.getCapabilities ? await gateway.getCapabilities() : null;
      setCaps(capabilities);
      setPhase('ready');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPhase('error');
    }
  }, [gateway]);

  useEffect(() => {
    void loadCapabilities();
  }, [loadCapabilities]);

  const tabs: { id: CapabilitiesTab; label: string }[] = [
    { id: 'skills', label: t(locale, 'capabilities.tabs.skills') },
    { id: 'connectors', label: t(locale, 'capabilities.tabs.connectors') },
    { id: 'experts', label: t(locale, 'capabilities.tabs.experts') },
  ];

  if (phase === 'loading') {
    return (
      <div className="flex h-full items-center justify-center">
        <LoadingState message={t(locale, 'capabilities.common.loading')} />
      </div>
    );
  }

  if (phase === 'error') {
    return (
      <div className="flex h-full items-center justify-center">
        <ErrorState
          message={error ?? t(locale, 'capabilities.gate.unavailableTitle')}
          onRetry={() => void loadCapabilities()}
        />
      </div>
    );
  }

  // Honest gate: daemon reachable but capability.* is not advertised yet.
  if (!canManageCapabilities(caps)) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState
          icon={<Blocks size={32} />}
          title={t(locale, 'capabilities.gate.unavailableTitle')}
          description={t(locale, 'capabilities.gate.unavailableDesc')}
          action={{
            label: t(locale, 'capabilities.common.retry'),
            onClick: () => void loadCapabilities(),
          }}
        />
      </div>
    );
  }

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', padding: SPACING.lg }}>
      <div className="mb-1 flex items-center gap-2">
        <Blocks size={18} style={{ color: 'var(--primary)' }} aria-hidden />
        <h1 style={{ fontSize: FONT_SIZE.xl, fontWeight: 600, color: 'var(--text)' }}>
          {t(locale, 'capabilities.title')}
        </h1>
      </div>
      <div
        role="tablist"
        aria-label={t(locale, 'capabilities.title')}
        style={{ display: 'flex', gap: 0, borderBottom: '0.0625rem solid var(--border)', marginBottom: SPACING.lg }}
      >
        {tabs.map((item) => (
          <button
            key={item.id}
            role="tab"
            aria-selected={tab === item.id}
            onClick={() => setTab(item.id)}
            style={{
              padding: `${SPACING.sm}px ${SPACING.md}px`,
              fontSize: FONT_SIZE.md,
              fontWeight: 500,
              background: 'none',
              border: 'none',
              borderBottom: tab === item.id ? '2px solid var(--primary)' : '2px solid transparent',
              color: tab === item.id ? 'var(--text)' : 'var(--text-secondary)',
              cursor: 'pointer',
            }}
          >
            {item.label}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        {tab === 'skills' && <SkillsTab locale={locale} gateway={gateway} />}
        {tab === 'connectors' && <ConnectorsTab locale={locale} gateway={gateway} caps={caps} />}
        {tab === 'experts' && <ExpertsTab locale={locale} gateway={gateway} />}
      </div>
    </div>
  );
}
