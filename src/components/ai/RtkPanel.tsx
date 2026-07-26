'use client';

import { t as tr, useLocale } from '@/i18n';
import { RefreshCw } from 'lucide-react';
import { ErrorState } from '@/components/ui/EmptyState';
import { useAsyncData } from '@/hooks/useAsyncData';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import type { UsageCacheReadResult } from '@/types/usage';

/**
 * RTK 节省统计 — 后端 RtkSummary 只有 totalSavedTokens / totalCommands 两个字段。
 * 旧版渲染的「Top commands」永远为空（后端不产出该字段），且「暂停追踪」按钮
 * 只翻转本地布尔、不暂停任何东西——均已删除（禁止假数据/假控件）。
 */

interface RtkSummary {
  totalSavedTokens: number;
  totalCommands: number;
}

export default function RtkPanel() {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  const { data, loading, error, reload } = useAsyncData<RtkSummary | null>(async () => {
    const api = window.nativesAPI;
    if (!api?.usage?.getCached) throw new Error('Usage API unavailable');
    const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
    const result = (await api.usage.getCached({
      preset: '30d',
      timeZone,
      projectPath: null,
    })) as UsageCacheReadResult;
    if (result.state === 'ready') {
      return ((result as { response?: { rtk?: RtkSummary } }).response?.rtk) ?? null;
    }
    return null;
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', padding: 'var(--space-sm)' }}>
      {/* Header */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        marginBottom: SPACING.md,
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t('aiWorkbench.rtkUsage')}
        </div>
        <button
          type="button"
          className="btn-ghost"
          onClick={reload}
          disabled={loading}
          title={t('common.refresh')}
          aria-label={t('common.refresh')}
          style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm, display: 'inline-flex' }}
        >
          {loading ? '…' : <RefreshCw size={12} />}
        </button>
      </div>

      {error ? (
        <ErrorState message={error.userMessage} onRetry={reload} />
      ) : (
        <div style={{ display: 'flex', gap: 'var(--space-sm)' }}>
          <div style={{
            flex: 1, padding: 10, borderRadius: BORDER_RADIUS.md,
            background: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 18, fontWeight: 700, color: 'var(--primary)' }}>
              {loading ? '…' : (data?.totalSavedTokens ?? 0).toLocaleString()}
            </div>
            <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', marginTop: 2 }}>
              {t('aiWorkbench.tokensSaved')}
            </div>
          </div>
          <div style={{
            flex: 1, padding: 10, borderRadius: BORDER_RADIUS.md,
            background: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 18, fontWeight: 700, color: 'var(--text)' }}>
              {loading ? '…' : (data?.totalCommands ?? 0).toLocaleString()}
            </div>
            <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', marginTop: 2 }}>
              {t('aiWorkbench.commands')}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
