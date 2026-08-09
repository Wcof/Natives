'use client';

/**
 * P2-03 · 兼容再导出桥（受控迁移，非业务实现）。
 *
 * 共享 preview UI 已物理迁移到 src/components/ui/preview/**（R-E3：纯展示层
 * 归共享 UI 原子，Feature 不复制预览算法）。本文件只把 ArtifactPreviewSurface
 * 再导出到旧路径，供尚在迁移中的 assistant 域（ActivityInspector）无感引用。
 * assistant 域完成 import 路径迁移后即可删除本文件（F3-03/Q3-01 收口）。
 */

export { default, type ArtifactPreviewSurfaceProps } from '@/components/ui/preview/ArtifactPreviewSurface';
