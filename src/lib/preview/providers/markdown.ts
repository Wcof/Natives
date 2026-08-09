// T12 · Markdown Preview Provider
//
// file source 必须先 ctx.authorizeFile(path) 再 ctx.readText；authorize/read 的
// fatal（permission/security/io）原样透传，禁止降级。memory source 不获得 FS 能力。
// urlPolicy 区分：file → authorized-file-assets（保留本地图片），memory → assistant-safe。
// SEC-001：文档内引用的本地图片同样逐个经 ctx.authorizeFile 授权后才改写为
// asset:// URL（rewriteAuthorizedLocalImages）；未授权引用不产出可访问 URL。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { rewriteAuthorizedLocalImages } from './markdown-policy';

const MD_EXTS = new Set(['md', 'markdown']);

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

export const markdownProvider: PreviewProvider = {
  id: 'markdown',
  priority: 100,
  accepts(request: PreviewRequest): boolean {
    // 非 text kind（image/pdf/archive/…）不可能是 Markdown 文档
    if (request.source.type === 'file' && request.source.kind && request.source.kind !== 'text') return false;
    return MD_EXTS.has(extOf(sourceName(request.source)));
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    if (request.source.type === 'memory') {
      return {
        kind: 'markdown',
        source: request.source.content,
        truncated: false,
        urlPolicy: 'assistant-safe',
      };
    }
    // fatal（permission_denied/security_violation/io_error）由 context 抛出并原样透传
    const file = await ctx.authorizeFile(request.source.path);
    const result = await ctx.readText(file);
    const lastSlash = file.path.lastIndexOf('/');
    const baseDir = lastSlash > 0 ? file.path.slice(0, lastSlash) : undefined;
    // SEC-001：逐资源授权 + 改写；未授权引用保持原文，不产出 asset:// URL
    const { text, authorizedAssets } = baseDir
      ? await rewriteAuthorizedLocalImages(result.content, baseDir, ctx.authorizeFile)
      : { text: result.content, authorizedAssets: [] };
    return {
      kind: 'markdown',
      source: text,
      truncated: result.truncated,
      baseDir,
      urlPolicy: 'authorized-file-assets',
      authorizedAssets,
    };
  },
};
