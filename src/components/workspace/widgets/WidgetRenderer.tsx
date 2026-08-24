'use client';

/**
 * WidgetRenderer（B-017）—— loading/error/ready 状态机。
 * 数据一律来自 WorkspaceDataBroker（def.load → domain query/facade），
 * Renderer 不直连 IPC，不读 SQLite。
 */

import { useCallback, useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { t, useLocale } from '@/i18n';
import { Skeleton, ErrorPrimitive } from '@/components/ui/design-system';
import {
  workspaceDataBroker,
  buildWidgetBrokerKey,
} from '@/lib/workspace/widgets/data-broker';
import type { BrokerSnapshot } from '@/lib/workspace/widgets/data-broker';
import { useWorkspaceTimeRange } from '@/lib/workspace/widgets/time-range-context';
import type { WidgetConfig, WidgetDefinition, WidgetInstance } from '@/lib/workspace/widgets';
import { WidgetShell } from './WidgetShell';

export interface WidgetRendererProps<
  TData = unknown,
  TSettings extends Record<string, unknown> = Record<string, unknown>,
> {
  instance: WidgetInstance<TData, TSettings>;
  /** 覆盖 broker key（默认由 def.adapterKeyBuilder / type 推导）。 */
  adapterKey?: string;
  /** 编辑态 chrome（host 透传）。 */
  editing?: boolean;
  onRemove?: () => void;
}

/** 默认 staleTime：15s（同一布局快速重挂载不重复请求）。 */
const WIDGET_STALE_MS = 15_000;

/**
 * 订阅 data-broker 的 hook。
 * 每次渲染直接读取 broker 快照（key 变化时无旧数据闪现）；
 * 无 load 的纯展示 Widget：直接 ready（data=null）。
 */
function useWidgetData<TData, TSettings extends Record<string, unknown>>(
  def: WidgetDefinition<TData, TSettings>,
  config: WidgetConfig<TSettings>,
  key: string,
): BrokerSnapshot<TData> & { retry: () => void } {
  // 仅用于触发重渲染；数据始终从 broker 最新快照读取。
  const [, forceRender] = useState(0);
  const snapshot = workspaceDataBroker.getSnapshot<TData>(key);

  useEffect(() => {
    let disposed = false;
    const onSnapshot = () => {
      if (!disposed) forceRender((n) => n + 1);
    };

    const unsub = workspaceDataBroker.subscribe<TData>(
      key,
      def.load
        ? async (ctx) => {
            const data = await def.load!(ctx);
            return data;
          }
        : null,
      {
        staleTimeMs: WIDGET_STALE_MS,
        onSnapshot,
        onEvent: def.subscribe
          ? (emit) => {
              const un = def.subscribe!(emit);
              return typeof un === 'function' ? un : () => {};
            }
          : undefined,
      },
    );

    return () => {
      disposed = true;
      unsub();
    };
  }, [key, def]);

  const retry = useCallback(() => {
    workspaceDataBroker.refetch(key);
  }, [key]);

  return { ...snapshot, retry };
}

export function WidgetRenderer<TData, TSettings extends Record<string, unknown>>({
  instance,
  adapterKey,
  editing = false,
  onRemove,
}: WidgetRendererProps<TData, TSettings>) {
  const locale = useLocale();
  const timeRange = useWorkspaceTimeRange();
  const { def, config } = instance;
  const key = adapterKey ?? buildWidgetBrokerKey(def, config, timeRange);
  const { status, data, error, retry, refetching } = useWidgetData(def, config, key);
  const unsupported = data != null && typeof data === 'object' && 'unavailable' in data && data.unavailable === true;

  let body: ReactNode;
  if (status === 'error' && error) {
    body = (
      <div className="ws-shell-state">
        <ErrorPrimitive
          message={error.message || t(locale, 'common.error')}
          onRetry={retry}
          retryLabel={t(locale, 'common.retry')}
        />
      </div>
    );
  } else if (status === 'loading' && data == null) {
    body = (
      <div className="ws-shell-state">
        <div style={{ width: '100%', maxWidth: 260 }}>
          <Skeleton variant="card" lines={3} />
        </div>
      </div>
    );
  } else if (unsupported) {
    body = <div className="ws-shell-state text-xs text-[var(--text-tertiary)]">{t(locale, 'workspace.dataUnavailable')}</div>;
  } else {
    const Component = def.Component;
    body = (
      <div className="relative h-full">
      {refetching && <span className="absolute right-2 top-1 z-10 rounded bg-[var(--surface-hover)] px-1.5 py-0.5 text-xs text-[var(--text-disabled)]">{t(locale, 'workspace.staleData')}</span>}
      <Component
        data={data}
        loading={status === 'loading' || status === 'idle'}
        error={error}
        retry={retry}
        config={config}
      />
      </div>
    );
  }

  return (
    <WidgetShell instance={instance} editing={editing} onRemove={onRemove}>
      {body}
    </WidgetShell>
  );
}

export default WidgetRenderer;
