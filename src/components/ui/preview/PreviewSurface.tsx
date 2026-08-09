'use client';

/**
 * T19 · PreviewSurface — 可复用只读预览 Surface（controller host）。
 *
 * 每个 Surface（files/assistant/artifact/follow）持有自己的
 * PreviewRequestController + AbortController（经 usePreview）：一个 Surface 的
 * 新请求不会让另一个 Surface 的请求 stale/cancel（R16/D8）。本组件不持有任何
 * 全局 generation，只把 usePreview 的状态投影为 loading/error/unsupported 呈现。
 *
 * edit/dirty/write 属于现有 Editor 路径，本 Surface 严格只读。
 */

import { useEffect } from 'react';
import { t, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { PreviewSource } from '@/lib/preview/contracts';
import { PreviewProviderError } from '@/lib/preview/errors';
import type { PreviewService } from '@/lib/preview/service';
import { usePreview, type PreviewStatus } from '@/hooks/usePreview';
import { PreviewRenderer } from './PreviewRenderer';

export interface PreviewSurfaceProps {
  source: PreviewSource;
  surface: 'files' | 'assistant' | 'artifact' | 'follow';
  service: PreviewService;
  /** 打开后立即加载（默认 true）；false 时等待 load()/reload() */
  autoLoad?: boolean;
  onStatusChange?: (status: PreviewStatus) => void;
}

export default function PreviewSurface({ source, surface, service, autoLoad = true, onStatusChange }: PreviewSurfaceProps) {
  const locale = useLocale();
  const { model, status, error } = usePreview({ source, surface, service, autoLoad });
  const sourceKey = source.type === 'file' ? source.path : source.name;

  useEffect(() => {
    onStatusChange?.(status);
  }, [status, onStatusChange]);

  if (status === 'loading') {
    return <div data-preview-status="loading" style={{ padding: 24, color: 'var(--text-secondary)' }}>{t(locale, 'preview.loading')}</div>;
  }

  if (status === 'error') {
    // PREV-006: 经错误分类器产出用户可见文案（userMessage/actionHint），
    // 原始 error.message（不得含本地绝对路径，见 context.ts）不进用户可见拷贝。
    const classified = classifyError(error ?? new PreviewProviderError('host_error', 'preview failed'), { locale });
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
