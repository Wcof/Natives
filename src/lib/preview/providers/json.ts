// T13 · JSON Preview Provider
//
// JSON.parse 失败只以 recoverable('parse_failed') 降级（允许 code fallback）；
// permission/security/io 类 fatal 绝不降级。预算覆盖 read/parse/render 三段：
//  - 字节预算：超过 JSON_MAX_PARSE_BYTES 的主线程不 parse 完整字符串，
//    只解析有界前缀（partial model），防止超大 JSON 打爆渲染线程（R-P2）；
//  - 节点/深度预算：解析后 countJsonNodes 限制渲染规模（R-P4）。
// 超限标记 truncated 而不是无限展开。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { recoverableError } from '../errors';

/** 节点预算与深度上限：防止超大 JSON 打爆 renderer（R12） */
export const JSON_NODE_BUDGET = 20_000;
export const JSON_DEPTH_BUDGET = 64;

/** 主线程单次 JSON.parse 的最大输入字节数（512KB）；超出只做有界前缀解析 */
export const JSON_MAX_PARSE_BYTES = 512 * 1024;

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

/** UTF-8 字节数（代理对计 4 字节；高代理缺失时按 3 字节保守计） */
export function utf8ByteLength(s: string): number {
  let bytes = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const low = s.charCodeAt(i + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        bytes += 4;
        i++;
        continue;
      }
    }
    bytes += c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
  }
  return bytes;
}

/** 截取前 byteBudget 字节，保证不在代理对中间切开 UTF-16 */
export function sliceUtf8(s: string, byteBudget: number): string {
  if (utf8ByteLength(s) <= byteBudget) return s;
  let bytes = 0;
  let end = 0;
  while (end < s.length) {
    const c = s.charCodeAt(end);
    if (c >= 0xd800 && c <= 0xdbff && end + 1 < s.length) {
      const low = s.charCodeAt(end + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        if (bytes + 4 > byteBudget) break;
        bytes += 4;
        end += 2;
        continue;
      }
    }
    const inc = c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
    if (bytes + inc > byteBudget) break;
    bytes += inc;
    end++;
  }
  // 防御：若 end 落在代理对之间，回退一位避免切出孤立半代理
  if (end < s.length && end > 0) {
    const c = s.charCodeAt(end);
    if (c >= 0xdc00 && c <= 0xdfff && s.charCodeAt(end - 1) >= 0xd800 && s.charCodeAt(end - 1) <= 0xdbff) {
      end--;
    }
  }
  return s.slice(0, end);
}

/**
 * 在有界前缀内闭合未完成的 JSON：扫描前缀，记录每个「完整值结束」的截断候选，
 * 从最后到最早尝试「截断 + 补齐未闭合容器」，返回第一个能通过 JSON.parse 的结果；
 * 找不到可闭合点返回 null（调用方以 recoverable parse_failed 降级）。
 * 只会在 JSON.parse(prefix) 失败（前缀被截断在结构中间）时被调用。
 */
export function closeJsonPrefix(prefix: string): string | null {
  const stack: Array<'{' | '['> = [];
  const expectKeyStack: boolean[] = [];
  const cuts: Array<{ index: number; stack: Array<'{' | '['> }> = [];
  let inStr = false;
  let esc = false;
  let isKey = false;
  let inValueToken = false;
  let valueEnded = false; // 刚结束一个完整值（string/container），等待分隔符/EOF 确认

  const recordCut = (end: number) => {
    if (stack.length === 0) return; // 根值位置不需要截断
    cuts.push({ index: end, stack: stack.slice() });
  };

  const finishValueToken = (end: number) => {
    if (inValueToken) {
      inValueToken = false;
      recordCut(end);
      valueEnded = true;
    }
  };

  for (let i = 0; i < prefix.length; i++) {
    const ch = prefix[i]!;
    if (inStr) {
      if (esc) {
        esc = false;
        continue;
      }
      if (ch === '\\') {
        esc = true;
        continue;
      }
      if (ch === '"') {
        inStr = false;
        if (isKey) {
          isKey = false; // key 结束，期望冒号：不记录截断
        } else {
          valueEnded = true;
        }
      }
      continue;
    }
    if (ch === '"') {
      inStr = true;
      esc = false;
      inValueToken = false;
      const top = stack.length > 0 ? stack[stack.length - 1] : null;
      isKey = top === '{' && expectKeyStack[expectKeyStack.length - 1] === true;
      valueEnded = false;
      continue;
    }
    if (ch === ' ' || ch === '\n' || ch === '\r' || ch === '\t') {
      finishValueToken(i);
      continue;
    }
    if (ch === ',') {
      finishValueToken(i);
      valueEnded = false;
      if (stack.length > 0 && stack[stack.length - 1] === '{') {
        expectKeyStack[expectKeyStack.length - 1] = true;
      }
      continue;
    }
    if (ch === ':') {
      finishValueToken(i);
      valueEnded = false;
      if (stack.length > 0) expectKeyStack[expectKeyStack.length - 1] = false;
      continue;
    }
    if (ch === '{' || ch === '[') {
      finishValueToken(i);
      valueEnded = false;
      stack.push(ch);
      expectKeyStack.push(ch === '{');
      continue;
    }
    if (ch === '}' || ch === ']') {
      finishValueToken(i);
      if (stack.length === 0) return null; // 多余闭合：不是合法 JSON 前缀
      const top = stack.pop()!;
      expectKeyStack.pop();
      if ((ch === '}' && top !== '{') || (ch === ']' && top !== '[')) return null;
      valueEnded = true; // 刚关闭的容器本身是一个完整值
      continue;
    }
    // 数字 / true / false / null 字面量 token
    if (!inValueToken) {
      if (valueEnded) recordCut(i); // 先前的完整值在此被下一个 token 分隔
      valueEnded = false;
      inValueToken = true;
    }
  }
  finishValueToken(prefix.length);
  if (valueEnded) recordCut(prefix.length);

  for (let k = cuts.length - 1; k >= 0; k--) {
    const { index, stack: stackAtCut } = cuts[k]!;
    const tail = prefix.slice(0, index);
    const closers = stackAtCut.slice().reverse().map((c) => (c === '{' ? '}' : ']')).join('');
    try {
      JSON.parse(tail + closers);
      return tail + closers;
    } catch {
      // 该截断点不能闭合，尝试更早的候选
    }
  }
  return null;
}

/**
 * 有界 JSON 解析：源字节数在预算内直接 parse；超出预算只解析有界前缀
 * （优先闭合出完整 partial model），无法闭合则以 recoverable parse_failed 降级。
 */
export function parseJsonBounded(source: string): { value: unknown; truncated: boolean } {
  if (utf8ByteLength(source) <= JSON_MAX_PARSE_BYTES) {
    return { value: JSON.parse(source), truncated: false };
  }
  const prefix = sliceUtf8(source, JSON_MAX_PARSE_BYTES);
  try {
    return { value: JSON.parse(prefix), truncated: true };
  } catch {
    const closed = closeJsonPrefix(prefix);
    if (closed !== null) return { value: JSON.parse(closed), truncated: true };
    throw recoverableError('parse_failed', 'json exceeds byte budget and cannot be partially parsed');
  }
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
    let byteTruncated = false;
    try {
      const parsed = parseJsonBounded(source);
      value = parsed.value;
      byteTruncated = parsed.truncated;
    } catch (error) {
      if (error instanceof Error && error.name === 'PreviewProviderError') throw error;
      throw recoverableError('parse_failed', 'invalid json');
    }
    const { nodeCount, truncated } = countJsonNodes(value);
    return {
      kind: 'json',
      value,
      formatted: JSON.stringify(value, null, 2),
      nodeCount,
      truncated: byteTruncated || truncated,
    };
  },
};
