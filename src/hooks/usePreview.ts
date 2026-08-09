'use client';

/**
 * usePreview — Preview Capability 唯一共享 Hook（T19 · 四 Surface 共用）。
 *
 * Files / Assistant / Artifact / Follow 各自的只读预览 surface 都经本 hook
 * 持有一个 PreviewRequestController：新请求只取消本 surface 的旧请求，互不
 * stale/cancel（R16）。loading/error/unsupported 状态在这里归一，renderer 只
 * 消费 PreviewModel。
 *
 * 用法：
 *   const service = useMemo(() => new PreviewService(createBuiltinRegistry(), createDefaultContext()), []);
 *   const { model, status, error, reload } = usePreview({ source, surface, service });
 *
 * reload() 用于 source 未变但需强制重读（如 follow 面板同一文件被 agent 重写）。
 * edit/dirty/write 属于现有 Editor 路径，本 hook 严格只读。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { PreviewModel, PreviewSource, PreviewSurfaceId } from '@/lib/preview/contracts';
import { PreviewProviderError } from '@/lib/preview/errors';
import { PreviewRequestController } from '@/lib/preview/request-controller';
import type { PreviewService } from '@/lib/preview/service';

export type PreviewStatus = 'loading' | 'ready' | 'error' | 'unsupported';

export interface UsePreviewOptions {
  source: PreviewSource;
  surface: PreviewSurfaceId;
  service: PreviewService;
  /** 挂载后立即加载（默认 true）；false 时由 load()/reload() 显式触发 */
  autoLoad?: boolean;
}

export interface UsePreviewResult {
  model: PreviewModel | null;
  status: PreviewStatus;
  error: PreviewProviderError | null;
  /** 重新发起当前 source 的请求（stale 结果会被丢弃） */
  load: () => void;
  /** 即使 source 未变也强制重读（文件被外部重写时用） */
  reload: () => void;
}

/** source 稳定键：相同 path/memory 内容不重复触发加载 */
export function sourceKeyOf(source: PreviewSource): string {
  if (source.type === 'file') {
    return [
      'file',
      source.path,
      source.kind ?? '',
      source.size ?? 0,
      source.mtime ?? 0,
    ].join(':');
  }
  return ['memory', source.name, source.content.length, source.mime ?? ''].join(':');
}

export function usePreview({ source, surface, service, autoLoad = true }: UsePreviewOptions): UsePreviewResult {
  // Surface-local controller：本 surface 的 stale/cancel 权威（永不重置）
  const controllerRef = useRef<PreviewRequestController | null>(null);
  if (!controllerRef.current) controllerRef.current = new PreviewRequestController();
  const controller = controllerRef.current;

  const [model, setModel] = useState<PreviewModel | null>(null);
  const [status, setStatus] = useState<PreviewStatus>('loading');
  const [error, setError] = useState<PreviewProviderError | null>(null);
  const [reloadTick, setReloadTick] = useState(0);

  const sourceKey = useMemo(() => sourceKeyOf(source), [source]);

  const load = useCallback(() => {
    const ctrl = controller;
    const { generation, signal } = ctrl.next();
    setStatus('loading');
    setError(null);
    const request = { source, mode: 'preview' as const, surface, signal };
    void service.prepare(request).then(
      (result) => {
        if (!ctrl.isCurrent(generation)) return; // 本 surface 已发起新请求 → 丢弃 stale
        if (result.kind === 'unsupported') {
          setModel(result);
          setStatus('unsupported');
        } else {
          setModel(result);
          setStatus('ready');
        }
      },
      (e: unknown) => {
        if (!ctrl.isCurrent(generation)) return;
        if (e instanceof PreviewProviderError && e.code === 'cancelled') return; // 静默
        // PREV-006：非分类意外错误不以 String(e)（可能含本地绝对路径）进入
        // error.message；分类器只消费结构化的 PreviewProviderError。
        setError(e instanceof PreviewProviderError ? e : new PreviewProviderError('host_error', 'preview failed'));
        setStatus('error');
      },
    );
  }, [source, surface, service, controller]);

  // loadRef 保持最新 load，effect 只按稳定键（sourceKey/surface/service/reloadTick）
  // 触发，避免父组件每次渲染传新 source 对象导致无限重载。
  const loadRef = useRef(load);
  loadRef.current = load;

  useEffect(() => {
    if (autoLoad) loadRef.current();
    return () => controller.cancel(); // 卸载时只取消本 surface 的请求
  }, [sourceKey, surface, service, autoLoad, controller, reloadTick]);

  const reload = useCallback(() => setReloadTick((n) => n + 1), []);

  return { model, status, error, load, reload };
}
