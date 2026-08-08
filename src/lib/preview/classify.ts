// Preview 能力分类（T10 · C0 frozen）
//
// 不重新实现 Rust detect_file_kind：file source 优先使用授权后的 Host kind，
// 再结合 extension 做 preview 特化。memory source 用 extension/mime 提示。

import type { FileKind } from '@/types/file';
import type { PreviewSource } from './contracts';

export type PreviewCapability =
  | 'markdown'
  | 'html'
  | 'json'
  | 'code'
  | 'image'
  | 'video'
  | 'audio'
  | 'pdf'
  | 'csv'
  | 'archive'
  | 'unsupported';

export function extOf(name: string): string {
  const base = name.split(/[?#]/)[0] ?? '';
  const dot = base.lastIndexOf('.');
  return dot >= 0 ? base.slice(dot + 1).toLowerCase() : '';
}

const TEXT_BY_EXT: Record<string, PreviewCapability> = {
  md: 'markdown',
  markdown: 'markdown',
  html: 'html',
  htm: 'html',
  json: 'json',
  csv: 'csv',
};

/** kind 为 image/video/audio/pdf/archive 时优先按 kind 分类（Host 权威） */
const KIND_CAPABILITY: Partial<Record<FileKind, PreviewCapability>> = {
  image: 'image',
  video: 'video',
  audio: 'audio',
  pdf: 'pdf',
  archive: 'archive',
};

/**
 * 分类入口：file source 用 kind（Host 已授权）→ ext 特化；memory source 用 name。
 * 纯函数，供 T12–T17 provider 复用；默认降级为 code（文本兜底）。
 */
export function classifyCapability(source: PreviewSource): PreviewCapability {
  const name = source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? source.path : 'memory');
  const ext = extOf(name);

  if (source.type === 'file' && source.kind) {
    const byKind = KIND_CAPABILITY[source.kind];
    if (byKind) return byKind;
    if (source.kind === 'text') {
      return TEXT_BY_EXT[ext] ?? 'code';
    }
    return 'unsupported';
  }

  // memory source（或 file source 未带 kind）：仅按 ext 提示，不冒充 Host 判定
  return TEXT_BY_EXT[ext] ?? 'code';
}
