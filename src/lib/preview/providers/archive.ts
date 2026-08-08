// T17 · Archive Preview Provider
//
// 把 IO 从 ArchivePreview 组件移入 provider：file source 必须先
// authorizeFile 再 listArchive；renderer 只消费 model。fatal 原样透传；
// 非压缩包列出失败 → recoverable unsupported（允许降级提示）。

import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { recoverableError } from '../errors';

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

const ARCHIVE_EXTS = new Set(['zip', 'tar', 'gz', 'tgz', '7z', 'rar', 'bz2', 'xz', 'zst']);

export const archiveProvider: PreviewProvider = {
  id: 'archive',
  priority: 70,
  accepts(request: PreviewRequest): boolean {
    if (request.source.type === 'memory') return false;
    if (request.source.kind === 'archive') return true;
    const ext = sourceName(request.source).split('.').pop()?.toLowerCase() ?? '';
    return ARCHIVE_EXTS.has(ext);
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    if (request.source.type !== 'file') {
      throw recoverableError('not_applicable', 'archive requires a file source');
    }
    // fatal（permission/security/io）由 context 抛出并原样透传
    const file = await ctx.authorizeFile(request.source.path);
    const listing = await ctx.listArchive(file);
    return {
      kind: 'archive',
      entries: listing.entries,
      truncated: listing.truncated,
    };
  },
};
