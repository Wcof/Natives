'use client';

/**
 * T40 · ArtifactPreviewSurface — Assistant/Agent 产物文件的只读预览 Surface。
 *
 * 复用 Preview V2 统一管线（builtin registry/service/context），surface='artifact'。
 * 每个实例自带 PreviewRequestController（PreviewSurface 内部持有），Files/Assistant/
 * Artifact/Follow 并发互不 stale/cancel。只读：不获得任何 mutation 权限（R14）。
 * 普通 chat Markdown 不迁移（继续走 SafeMarkdown 最短路径）。
 */

import { useMemo } from 'react';
import type { PreviewSource } from '@/lib/preview/contracts';
import { createBuiltinRegistry, createDefaultContext } from '@/lib/preview/composition';
import { PreviewService } from '@/lib/preview/service';
import PreviewSurface from './PreviewSurface';

export interface ArtifactPreviewSurfaceProps {
  source: PreviewSource;
  autoLoad?: boolean;
  onStatusChange?: (status: 'loading' | 'ready' | 'error' | 'unsupported') => void;
}

export default function ArtifactPreviewSurface({ source, autoLoad = true, onStatusChange }: ArtifactPreviewSurfaceProps) {
  const service = useMemo(
    () => new PreviewService(createBuiltinRegistry(), createDefaultContext()),
    [],
  );
  return (
    <PreviewSurface
      source={source}
      surface="artifact"
      service={service}
      autoLoad={autoLoad}
      onStatusChange={onStatusChange}
    />
  );
}
