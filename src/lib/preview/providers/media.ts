// T14 · Media Preview Provider（image/video/audio）
//
// file source 必须先 ctx.authorizeFile(path) 再 ctx.toAssetUrl(AuthorizedPreviewFile)；
// 禁止 raw path 直接 toAssetUrl/convertFileSrc（R14/D10）。authorize 抛出的
// fatal（permission/security/io）原样透传，绝不降级。HEIC/TIFF 等受控转换没有
// 宿主 convert 钩子，不提供未接线的半成品 public type（整改原则：不保留
// 未实现的声明面）。
// PREV-004：Host kind（image/video/audio）优先，扩展名其次；未知扩展名
// 不允许误判 image（旧实现 `kind in EXT_TO_KIND` 用错 key 空间导致 kind 分支失效，
// 且未知扩展名回退到 'image'）。

import type { FileKind } from '@/types/file';
import { extOf } from '../classify';
import type {
  PreviewContext,
  PreviewModel,
  PreviewProvider,
  PreviewRequest,
} from '../contracts';
import { recoverableError } from '../errors';

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

const EXT_TO_KIND: Record<string, 'image' | 'video' | 'audio'> = {
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  svg: 'image',
  bmp: 'image',
  heic: 'image',
  heif: 'image',
  tiff: 'image',
  tif: 'image',
  mp4: 'video',
  webm: 'video',
  mov: 'video',
  mkv: 'video',
  mp3: 'audio',
  wav: 'audio',
  ogg: 'audio',
  m4a: 'audio',
  flac: 'audio',
};

/** PREV-004：仅 Host 的 media kind 算数；其余 kind（text/pdf/…）一律不算 */
function isMediaKind(kind: unknown): kind is 'image' | 'video' | 'audio' {
  return kind === 'image' || kind === 'video' || kind === 'audio';
}

/**
 * PREV-004：Host kind 优先；未知扩展名 → not_applicable，绝不回退 image。
 * accepts() 已保证非 media kind 时扩展名在 EXT_TO_KIND 内，此处是防御性收口。
 */
function resolveMediaKind(
  hostKind: FileKind | undefined,
  ext: string,
): 'image' | 'video' | 'audio' {
  if (isMediaKind(hostKind)) return hostKind;
  const byExt = EXT_TO_KIND[ext];
  if (byExt) return byExt;
  throw recoverableError('not_applicable', `media provider cannot determine kind for ".${ext}"`);
}

export const mediaProvider: PreviewProvider = {
  id: 'media',
  priority: 80,
  accepts(request: PreviewRequest): boolean {
    if (request.source.type === 'memory') return false;
    // Host kind（image/video/audio）优先；text kind 且扩展名可识别时也接受
    if (isMediaKind(request.source.kind)) return true;
    return extOf(sourceName(request.source)) in EXT_TO_KIND;
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    if (request.source.type !== 'file') {
      throw recoverableError('not_applicable', 'media requires a file source');
    }
    // fatal（permission_denied/security_violation/io_error）由 context 抛出并原样透传
    const file = await ctx.authorizeFile(request.source.path);
    const kind = resolveMediaKind(request.source.kind, extOf(sourceName(request.source)));
    return {
      kind,
      src: ctx.toAssetUrl(file),
      name: file.name,
    };
  },
};
