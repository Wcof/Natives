'use client';

/**
 * T19 · PreviewSurface — 可复用只读预览 Surface（controller host）。
 *
 * 每个 Surface（files/assistant/artifact/follow）持有自己的
 * PreviewRequestController + AbortController：一个 Surface 的新请求不会让
 * 另一个 Surface 的请求 stale/cancel（R16/D8）。本组件不持有任何全局 generation。
 *
 * loading/error/unsupported 在这里显式呈现；renderer 只消费 PreviewModel。
 * edit/dirty/write 属于现有 Editor 路径，本 Surface 严格只读。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { t, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { PreviewModel, PreviewRequest, PreviewSource } from '@/lib/preview/contracts';
import { PreviewRequestController } from '@/lib/preview/request-controller';
import type { PreviewService } from '@/lib/preview/service';
import { PreviewProviderError } from '@/lib/preview/errors';
import { PreviewRenderer } from './PreviewRenderer';

export type PreviewStatus = 'loading' | 'ready' | 'error' | 'unsupported';

export interface PreviewSurfaceProps {
  source: PreviewSource;
  surface: 'files' | 'assistant' | 'artifact' | 'follow';
  service: PreviewService;
  /** 打开后立即加载（默认 true）；false 时等待 trigger() */
  autoLoad?: boolean;
  onStatusChange?: (status: PreviewStatus) => void;
}

export default function PreviewSurface({ source, surface, service, autoLoad = true, onStatusChange }: PreviewSurfaceProps) {
  const locale = useLocale();
  // Surface-local controller：本 Surface 的 stale/cancel 权威
  const controllerRef = useRef<PreviewRequestController | null>(null);
  if (!controllerRef.current) controllerRef.current = new PreviewRequestController();
  const controller = controllerRef.current;

  const [model, setModel] = useState<PreviewModel | null>(null);
  const [status, setStatus] = useState<PreviewStatus>('loading');
  const [error, setError] = useState<PreviewProviderError | null>(null);
  const sourceKey = source.type === 'file' ? source.path : source.name;

  const load = useCallback(async () => {
    const ctrl = controller;
    const { generation, signal } = ctrl.next();
    setStatus('loading');
    setError(null);
    const request: PreviewRequest = { source, mode: 'preview', surface, signal };
    try {
      const result = await service.prepare(request);
      if (!ctrl.isCurrent(generation)) return; // 本 Surface 已发起新请求 → 丢弃 stale
      if (result.kind === 'unsupported') {
        setModel(result);
        setStatus('unsupported');
      } else {
        setModel(result);
        setStatus('ready');
      }
    } catch (e) {
      if (!ctrl.isCurrent(generation)) return;
      if (e instanceof PreviewProviderError && e.code === 'cancelled') return; // 静默
      // PREV-006：非 PreviewProviderError 的意外错误不把 String(e)（可能含本地
      // 绝对路径）带进 error.message——classifyError 的 UNKNOWN 分支会把 rawMessage
      // 拼进用户可见文案，因此 message 必须在源头保证无路径。
      setError(e instanceof PreviewProviderError ? e : new PreviewProviderError('host_error', 'preview failed'));
      setStatus('error');
    }
  }, [source, surface, service, controller]);

  useEffect(() => {
    if (autoLoad) void load();
    return () => controller.cancel(); // 卸载时只取消本 Surface 的请求
  }, [sourceKey, surface, service, autoLoad, controller, load]);

  useEffect(() => {
    onStatusChange?.(status);
  }, [status, onStatusChange]);

  if (status === 'loading') {
    return <div data-preview-status="loading" style={{ padding: 24, color: 'var(--text-secondary)' }}>{t(locale, 'preview.loading')}</div>;
  }

  if (status === 'error') {
    // PREV-006: 经错误分类器产出用户可见文案（userMessage/actionHint），
    // 原始 error.message（不得含本地绝对路径，见 context.ts）不进用户可见拷贝。
    const classified = classifyError(error, { locale });
    return (
      <div data-preview-status="error" data-error-code={error?.code} style={{ padding: 24, color: 'var(--danger)' }}>
        <div>{classified.userMessage}</div>
        {classified.actionHint ? (
          <div style={{ color: 'var(--text-secondary)', fontSize: 12, marginTop: 4 }}>{classified.actionHint}</div>
        ) : null}
      </div>
    );
  }

  if (status === 'unsupported' || (model && model.kind === 'unsupported')) {
    return (
      <div data-preview-status="unsupported" style={{ padding: 24, color: 'var(--text-secondary)' }}>
        {model && model.kind === 'unsupported' ? model.reason : t(locale, 'preview.unsupported')}
      </div>
    );
  }

  if (!model) return null;

  return (
    <div data-preview-surface={surface} data-preview-source={sourceKey} style={{ height: '100%', overflow: 'auto' }}>
      <PreviewRenderer model={model} />
    </div>
  );
}
