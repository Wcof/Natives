// T13 · Code Preview Provider（文本兜底）
//
// 低优先级兜底：text kind 或 memory source 都接受。file source 同样必须先
// authorizeFile 再 readText；fatal 错误原样透传，不允许 catch-all 掩盖。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

export const codeProvider: PreviewProvider = {
  id: 'code',
  priority: 10,
  accepts(request: PreviewRequest): boolean {
    if (request.source.type === 'memory') return true;
    if (request.source.kind && request.source.kind !== 'text') return false;
    return true;
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    let source: string;
    let truncated = false;
    if (request.source.type === 'memory') {
      source = request.source.content;
    } else {
      const file = await ctx.authorizeFile(request.source.path);
      const result = await ctx.readText(file);
      source = result.content;
      truncated = result.truncated;
    }
    const name = sourceName(request.source);
    return { kind: 'code', source, language: extOf(name) || 'text', truncated };
  },
};
