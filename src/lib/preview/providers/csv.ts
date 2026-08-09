// T16 · CSV Preview Provider
//
// IO/parse 向 provider 移，renderer 只消费 model。file source 必须先
// authorizeFile 再 readText；fatal 原样透传。CSV 解析覆盖引号/换行（R11）。
// 预算覆盖 read/parse/render（R-P2/R-P4）：字节、行数、列数、单元格长度全部
// 有界，超出标记 truncated；解析输入受字节预算约束，不先 parse 完整文件再 slice。
// parse 失败仅 recoverable（unsupported → code 兜底）。

import { extOf } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { recoverableError } from '../errors';

/** 展示预算：最多解析 1000 行、单单元格最长 10000 字符，超出标 truncated（R-P4） */
export const CSV_MAX_ROWS = 1000;
export const CSV_MAX_CELL_LENGTH = 10_000;
/** 列预算：单行超过 200 列截断（R-P4） */
export const CSV_MAX_COLUMNS = 200;
/** 解析字节预算：主线程单次 parse 的输入上限（与 Host 截断读 256KB 对齐） */
export const CSV_MAX_PARSE_BYTES = 256 * 1024;

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

/** 近似 UTF-8 字节数（每 code unit 计；代理对略超计，预算方向保守） */
function utf8BytesOfCharCode(code: number): number {
  return code < 0x80 ? 1 : code < 0x800 ? 2 : 3;
}

/**
 * 解析整个 CSV 文本（含引号内换行），返回 headers/rows/truncated。
 * 预算：字节（CSV_MAX_PARSE_BYTES，超限即停）、行（CSV_MAX_ROWS）、列（CSV_MAX_COLUMNS）。
 */
export function parseCsv(content: string, delimiter = ','): ParsedCsv {
  const rows: string[][] = [];
  let truncated = false;
  let buffer = '';
  let inQuotes = false;
  let bytes = 0;
  let overBudget = false;

  const flushLine = (line: string) => {
    if (rows.length >= CSV_MAX_ROWS) {
      truncated = true;
      return;
    }
    const fields = parseCsvLine(line, delimiter);
    if (fields.length > CSV_MAX_COLUMNS) {
      fields.length = CSV_MAX_COLUMNS;
      truncated = true;
    }
    rows.push(fields);
  };

  for (let i = 0; i < content.length; i++) {
    if (bytes >= CSV_MAX_PARSE_BYTES) {
      overBudget = true;
      truncated = true;
      break;
    }
    const ch = content[i]!;
    bytes += utf8BytesOfCharCode(ch.charCodeAt(0));
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
  if (!overBudget && buffer.trim() !== '') flushLine(buffer);
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
