// T16 · CSV Preview Provider
//
// IO/parse 向 provider 移，renderer 只消费 model。file source 必须先
// authorizeFile 再 readText；fatal 原样透传。CSV 解析覆盖引号/换行（R11），
// 行数/列宽有界（R-P4）。parse 失败仅 recoverable（unsupported → code 兜底）。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { recoverableError } from '../errors';

/** 展示预算：最多解析 1000 行、单单元格最长 10000 字符，超出标 truncated（R-P4） */
export const CSV_MAX_ROWS = 1000;
export const CSV_MAX_CELL_LENGTH = 10_000;

export interface ParsedCsv {
  headers: string[];
  rows: string[][];
  truncated: boolean;
}

/** 支持引号包裹与换行的 CSV 行解析（RFC 4180 子集）：返回字段数组 */
export function parseCsvLine(line: string, delimiter = ','): string[] {
  const fields: string[] = [];
  let current = '';
  let inQuotes = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i]!;
    if (inQuotes) {
      if (ch === '"') {
        if (line[i + 1] === '"') {
          current += '"';
          i++;
        } else {
          inQuotes = false;
        }
      } else {
        current += ch;
      }
    } else if (ch === '"') {
      inQuotes = true;
    } else if (ch === delimiter) {
      fields.push(current);
      current = '';
    } else {
      current += ch;
    }
  }
  fields.push(current);
  return fields.map((f) => f.slice(0, CSV_MAX_CELL_LENGTH));
}

/** 解析整个 CSV 文本（含引号内换行），返回 headers/rows/truncated */
export function parseCsv(content: string, delimiter = ','): ParsedCsv {
  const rows: string[][] = [];
  let truncated = false;
  let buffer = '';
  let inQuotes = false;

  const flushLine = (line: string) => {
    if (rows.length >= CSV_MAX_ROWS) {
      truncated = true;
      return;
    }
    rows.push(parseCsvLine(line, delimiter));
  };

  for (let i = 0; i < content.length; i++) {
    const ch = content[i]!;
    if (ch === '"') {
      inQuotes = !inQuotes;
      buffer += ch;
    } else if (ch === '\n' && !inQuotes) {
      flushLine(buffer);
      buffer = '';
    } else {
      buffer += ch;
    }
  }
  if (buffer.trim() !== '') flushLine(buffer);
  if (inQuotes) truncated = true; // 未闭合引号：视为格式异常但仍可展示

  if (rows.length === 0) return { headers: [], rows: [], truncated };
  return { headers: rows[0]!, rows: rows.slice(1), truncated };
}

function sourceName(source: PreviewRequest['source']): string {
  return source.name ?? (source.type === 'file' ? source.path.split('/').pop() ?? '' : '');
}

export const csvProvider: PreviewProvider = {
  id: 'csv',
  priority: 85,
  accepts(request: PreviewRequest): boolean {
    if (request.source.type === 'file' && request.source.kind && request.source.kind !== 'text') return false;
    return extOf(sourceName(request.source)) === 'csv';
  },
  async prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel> {
    let source: string;
    if (request.source.type === 'memory') {
      source = request.source.content;
    } else {
      // fatal（permission/security/io）由 context 抛出并原样透传
      const file = await ctx.authorizeFile(request.source.path);
      const result = await ctx.readText(file);
      source = result.content;
    }
    try {
      const { headers, rows, truncated } = parseCsv(source);
      if (headers.length === 0) throw recoverableError('unsupported', 'csv has no header row');
      return { kind: 'csv', headers, rows, truncated };
    } catch (error) {
      if (error instanceof Error && error.name === 'PreviewProviderError') throw error;
      throw recoverableError('parse_failed', 'invalid csv');
    }
  },
};
