// T13 · JSON Preview Provider
//
// JSON.parse 失败只以 recoverable('parse_failed') 降级（允许 code fallback）；
// permission/security/io 类 fatal 绝不降级。大 JSON 带 node/depth budget，
// 超限标记 truncated 而不是无限展开（R-P4/R12）。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { recoverableError } from '../errors';

/** 节点预算与深度上限：防止超大 JSON 打爆 renderer（R12） */
export const JSON_NODE_BUDGET = 20_000;
export const JSON_DEPTH_BUDGET = 64;

export function countJsonNodes(value: unknown, depth = 0): { nodeCount: number; truncated: boolean } {
  if (depth > JSON_DEPTH_BUDGET) return { nodeCount: 1, truncated: true };
  let count = 1;
  let truncated = false;
  if (Array.isArray(value)) {
    for (const item of value) {
      const child = countJsonNodes(item, depth + 1);
      count += child.nodeCount;
      truncated = truncated || child.truncated;
      if (count > JSON_NODE_BUDGET) return { nodeCount: count, truncated: true };
    }
  } else if (value !== null && typeof value === 'object') {
    for (const key of Object.keys(value as Record<string, unknown>)) {
      const child = countJsonNodes((value as Record<string, unknown>)[key], depth + 1);
      count += child.nodeCount;
      truncated = truncated || child.truncated;
      if (count > JSON_NODE_BUDGET) return { nodeCount: count, truncated: true };
    }
  }
  return { nodeCount: count, truncated };
}

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

export const jsonProvider: PreviewProvider = {
  id: 'json',
  priority: 90,
  accepts(request: PreviewRequest): boolean {
    if (request.source.type === 'file' && request.source.kind && request.source.kind !== 'text') return false;
    return extOf(sourceName(request.source)) === 'json';
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    let source: string;
    if (request.source.type === 'memory') {
      source = request.source.content;
    } else {
      // fatal（permission/security/io）由 context 抛出并原样透传，绝不降级为 code
      const file = await ctx.authorizeFile(request.source.path);
      const result = await ctx.readText(file);
      source = result.content;
    }
    let value: unknown;
    try {
      value = JSON.parse(source);
    } catch {
      throw recoverableError('parse_failed', 'invalid json');
    }
    const { nodeCount, truncated } = countJsonNodes(value);
    return {
      kind: 'json',
      value,
      formatted: JSON.stringify(value, null, 2),
      nodeCount,
      truncated,
    };
  },
};
