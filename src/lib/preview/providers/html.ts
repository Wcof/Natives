// T20 · HTML Preview Provider
//
// HTML lane 垂直链（PREV-001）：provider → PreviewContext.prepareHtml（Host
// 授权 + /fs/{token}/ 逐资源重写）→ PreviewModel { kind:'html', html, sandbox }。
// file source 必须先 ctx.authorizeFile(path) 再 ctx.prepareHtml(file)；fatal
// （permission/security/io/host）原样透传，绝不降级。
//
// 沙箱红线（R-S2）：sandbox = allow-scripts allow-forms；无 allow-same-origin /
// allow-top-navigation / allow-popups。HTML 预览不接 Workshop Bridge token——
// /fs/{token}/ 是 Host 侧 preview session，与 Bridge token 域完全分离。
//
// 渲染以 srcDoc 承载（html 字段）：Host 已把所有本地引用改写为绝对
// http://localhost:{port}/fs/{token}/… 地址，renderer 无需再自行改写。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';

/** iframe sandbox 红线（R-S2）：脚本+表单；禁止 same-origin / top-navigation / popups */
export const HTML_PREVIEW_SANDBOX = 'allow-scripts allow-forms';

const HTML_EXTS = new Set(['html', 'htm']);

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

export const htmlProvider: PreviewProvider = {
  id: 'html',
  priority: 100,
  accepts(request: PreviewRequest): boolean {
    // 非 text kind（image/pdf/archive/…）不可能是 HTML 文档
    if (request.source.type === 'file' && request.source.kind && request.source.kind !== 'text') return false;
    return HTML_EXTS.has(extOf(sourceName(request.source)));
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    if (request.source.type === 'memory') {
      // 内存 HTML（assistant 生成的代码片段）：无 FS 能力，sandbox 直接呈现
      return {
        kind: 'html',
        revision: `memory:${request.source.name}:${request.source.content.length}`,
        html: request.source.content,
        sandbox: HTML_PREVIEW_SANDBOX,
      };
    }
    // fatal（permission_denied/security_violation/io_error/host_error）由
    // context 抛出并原样透传；未授权文件绝不会进入 prepareHtml。
    const file = await ctx.authorizeFile(request.source.path);
    const prepared = await ctx.prepareHtml(file);
    return {
      kind: 'html',
      revision: `${file.path}:${file.mtime}:${file.size}`,
      // Host 已重写本地引用为 /fs/{token}/…；renderer 以 srcDoc 呈现即可
      html: prepared.content,
      sandbox: HTML_PREVIEW_SANDBOX,
    };
  },
};
