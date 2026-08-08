// T14 · PDF Preview Provider
//
// file source 必须先 ctx.authorizeFile(path) 再 ctx.toAssetUrl(AuthorizedPreviewFile)。
// authorize 的 fatal 原样透传，不降级；memory source 不支持（not_applicable）。

import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { recoverableError } from '../errors';

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

export const pdfProvider: PreviewProvider = {
  id: 'pdf',
  priority: 75,
  accepts(request: PreviewRequest): boolean {
    if (request.source.type === 'memory') return false;
    if (request.source.kind === 'pdf') return true;
    return sourceName(request.source).toLowerCase().endsWith('.pdf');
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    if (request.source.type !== 'file') {
      throw recoverableError('not_applicable', 'pdf requires a file source');
    }
    // fatal（permission/security/io）由 context 抛出并原样透传
    const file = await ctx.authorizeFile(request.source.path);
    return { kind: 'pdf', src: ctx.toAssetUrl(file), name: file.name };
  },
};
