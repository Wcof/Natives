// T14 · Media Preview Provider（image/video/audio）
//
// file source 必须先 ctx.authorizeFile(path) 再 ctx.toAssetUrl(AuthorizedPreviewFile)；
// 禁止 raw path 直接 toAssetUrl/convertFileSrc（R14/D10）。authorize 抛出的
// fatal（permission/security/io）原样透传，绝不降级。HEIC/TIFF 等受控转换仅当
// 宿主提供 convert 钩子时接线，且转换只针对已授权文件。

import type {
  AuthorizedPreviewFile,
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

export const mediaProvider: PreviewProvider = {
  id: 'media',
  priority: 80,
  accepts(request: PreviewRequest): boolean {
    if (request.source.type === 'memory') return false;
    // Host kind（image/video/audio）优先；text kind 且扩展名可识别时也接受
    if (request.source.kind === 'image' || request.source.kind === 'video' || request.source.kind === 'audio') {
      return true;
    }
    const ext = sourceName(request.source).split('.').pop()?.toLowerCase() ?? '';
    return ext in EXT_TO_KIND;
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    if (request.source.type !== 'file') {
      throw recoverableError('not_applicable', 'media requires a file source');
    }
    // fatal（permission_denied/security_violation/io_error）由 context 抛出并原样透传
    const file = await ctx.authorizeFile(request.source.path);
    const kind = request.source.kind && request.source.kind !== 'text' && request.source.kind in EXT_TO_KIND
      ? (request.source.kind as 'image' | 'video' | 'audio')
      : EXT_TO_KIND[sourceName(request.source).split('.').pop()?.toLowerCase() ?? ''] ?? 'image';
    return {
      kind: kind === 'video' ? 'video' : kind === 'audio' ? 'audio' : 'image',
      src: ctx.toAssetUrl(file),
      name: file.name,
    };
  },
};

/** 供测试/后续接入的受控转换钩子类型（HEIC/TIFF → 缓存 jpeg；仅已授权文件） */
export type MediaConvertHook = (file: AuthorizedPreviewFile) => Promise<string | null>;
