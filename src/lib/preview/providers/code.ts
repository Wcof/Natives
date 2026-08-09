// T13 · Code Preview Provider（文本兜底）
//
// 低优先级兜底：text kind 或 memory source 都接受。file source 同样必须先
// authorizeFile 再 readText；fatal 错误原样透传，不允许 catch-all 掩盖。
// 预算覆盖 read/parse/render：行数（CODE_MAX_LINES）与字符数（CODE_MAX_CHARS）
// 在 provider 侧截断，renderer 只拿到有界 source，不把完整文件塞进 DOM（R-P4）。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';

/** 展示预算：最多保留 2000 行（provider 侧行预算，R-P4） */
export const CODE_MAX_LINES = 2000;
/** 防御性字符预算：memory source 不受 Host 截断保护，在此强制有界 */
export const CODE_MAX_CHARS = 256 * 1024;

/** 对源码做行/字符截断；返回截断后文本与是否超预算 */
export function sliceCodeBounded(
  source: string,
  maxLines = CODE_MAX_LINES,
  maxChars = CODE_MAX_CHARS,
): { source: string; truncated: boolean } {
  let text = source;
  let truncated = false;
  if (text.length > maxChars) {
    text = text.slice(0, maxChars);
    truncated = true;
  }
  const lines = text.split('\n');
  if (lines.length > maxLines) {
    text = lines.slice(0, maxLines).join('\n');
    truncated = true;
  }
  return { source: text, truncated };
}

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
    const bounded = sliceCodeBounded(source);
    const name = sourceName(request.source);
    return { kind: 'code', source: bounded.source, language: extOf(name) || 'text', truncated: truncated || bounded.truncated };
  },
};
