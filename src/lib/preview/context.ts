// PreviewContext 默认实现（T10 · C0 frozen）
//
// PreviewContext 不是第二个 Host Facade：它是 provider 完成「只读准备」所需的
// 最小能力注入。默认实现组合现有文件域入口（files-api），并按 T21 约定预留
// htmlPreviewApi 接入点。file source 必须先 authorizeFile 才能 read/asset/html/list。
//
// PREV-005：Host wire 数据在边界处经 zod schema 校验后才进入 provider/renderer，
// 禁止裸 as cast 穿透（对照 theme-engine.validateTheme 先例）。
// PREV-006：fatal 错误的 message 一律不含本地绝对路径——路径细节只由
// PreviewDiagnostics 在请求级记录（sanitize 后的诊断 buffer），不进用户可见文案。

import { z } from 'zod';
import type { ArchiveEntry, ReadFileResult } from '@/types/file';
import { archiveApi, fsApi } from '@/lib/files-api';
import type { AuthorizedPreviewFile, HtmlPreviewPrepared, PreviewContext } from './contracts';
import { PreviewProviderError, fatalError } from './errors';

/** fs.stat 的真实 wire 形态（tauri-adapter，path 可缺省） */
export type PreviewStatShape = {
  found: boolean;
  path?: string;
  isDir?: boolean;
  name?: string;
  kind?: string;
  size?: number;
  mtime?: number;
};

/** archive.list 的真实 wire 形态（不含 totalSize，由 listArchive 归一化补齐） */
export type PreviewArchiveShape = {
  entries: Array<{ name: string; size: number; isDir?: boolean }>;
  truncated: boolean;
};

// ── Host wire runtime validation（PREV-005）────────────────────────────
// 与 theme-engine.validateTheme 同一 zod 先例：wire 数据不可信，边界处 parse。
// 非法 wire → host_error（结构化 fatal）；zod 细节/值（可能含文件内容或路径）
// 绝不拼进 message。

const StatWireSchema = z.object({
  found: z.boolean(),
  path: z.string().optional(),
  isDir: z.boolean().optional(),
  name: z.string().optional(),
  kind: z.enum(['text', 'image', 'video', 'audio', 'pdf', 'archive', 'dir', 'other']).optional(),
  size: z.number().optional(),
  mtime: z.number().optional(),
});

const ReadFileResultWireSchema = z.object({
  content: z.string(),
  truncated: z.boolean(),
  size: z.number(),
  mtime: z.number(),
  kind: z.string(),
  encoding: z.string(),
});

const HtmlPreviewPreparedWireSchema = z.object({
  content: z.string(),
  fsBase: z.string(),
  serverPort: z.number(),
});

const ArchiveEntryWireSchema = z.object({
  name: z.string(),
  size: z.number(),
  isDir: z.boolean().optional(),
});

const ArchiveListingWireSchema = z.object({
  entries: z.array(ArchiveEntryWireSchema),
  truncated: z.boolean(),
});

/** 边界校验：畸形 wire → 结构化 host_error（message 不含路径/值细节） */
function parseWire<T>(schema: z.ZodType<T>, raw: unknown, what: string): T {
  const result = schema.safeParse(raw);
  if (!result.success) {
    throw fatalError('host_error', `host returned malformed wire data for ${what}`);
  }
  return result.data;
}

/** context 依赖的 Host 能力（窄接口，便于测试注入与 T21 统一接线） */
export interface PreviewHost {
  stat(path: string): Promise<PreviewStatShape>;
  readFile(path: string): Promise<unknown>;
  listArchive(path: string): Promise<PreviewArchiveShape>;
  toAssetUrl(path: string): string;
  prepareHtml(path: string): Promise<HtmlPreviewPrepared>;
}

/**
 * 生产默认 host：走 files-api 唯一入口；htmlPreview 属顶层 nativesAPI（T21 收敛到 files-api）。
 * 经全局 Window['nativesAPI'] 单一类型源取用，禁止再手写第二份内联定义（PREV-005）。
 */
export function defaultPreviewHost(): PreviewHost {
  return {
    stat: (path) => fsApi().stat(path),
    readFile: (path) => fsApi().readFile(path),
    listArchive: (path) => archiveApi().list(path),
    // 只有已授权文件会到达这里（PreviewContext.toAssetUrl 只在 authorize 后暴露）
    toAssetUrl: (path) => fsApi().convertFileSrc(path),
    prepareHtml: (path) => {
      const htmlPreview = typeof window !== 'undefined' ? window.nativesAPI?.htmlPreview : undefined;
      if (!htmlPreview) throw fatalError('host_error', 'htmlPreview API not available');
      return htmlPreview.prepare(path);
    },
  };
}

export function createPreviewContext(host: PreviewHost = defaultPreviewHost()): PreviewContext {
  return {
    /** 授权：Host stat → found && !isDir → AuthorizedPreviewFile；否则致命，绝不静默降级 */
    async authorizeFile(path: string): Promise<AuthorizedPreviewFile> {
      let raw: unknown;
      try {
        raw = await host.stat(path);
      } catch {
        throw fatalError('io_error', 'stat failed');
      }
      const stat = parseWire(StatWireSchema, raw, 'stat');
      if (!stat.found) {
        throw fatalError('permission_denied', 'path not accessible');
      }
      if (stat.isDir) {
        throw fatalError('security_violation', 'directory cannot be previewed as a file');
      }
      return {
        path: stat.path ?? path,
        name: stat.name ?? path.split('/').pop() ?? path,
        kind: stat.kind ?? 'other',
        size: stat.size ?? 0,
        mtime: stat.mtime ?? 0,
      };
    },

    async readText(file: AuthorizedPreviewFile): Promise<ReadFileResult> {
      let raw: unknown;
      try {
        raw = await host.readFile(file.path);
      } catch {
        throw fatalError('io_error', 'read failed');
      }
      return parseWire(ReadFileResultWireSchema, raw, 'readFile');
    },

    toAssetUrl(file: AuthorizedPreviewFile): string {
      // 只有已授权文件允许生成 asset URL；禁止 raw path -> convertFileSrc
      return host.toAssetUrl(file.path);
    },

    async prepareHtml(file: AuthorizedPreviewFile): Promise<HtmlPreviewPrepared> {
      let raw: unknown;
      try {
        raw = await host.prepareHtml(file.path);
      } catch (error) {
        if (error instanceof PreviewProviderError) throw error;
        throw fatalError('host_error', 'html prepare failed');
      }
      return parseWire(HtmlPreviewPreparedWireSchema, raw, 'prepareHtml');
    },

    async listArchive(file: AuthorizedPreviewFile): Promise<{ entries: ArchiveEntry[]; truncated: boolean; totalSize: number }> {
      let raw: unknown;
      try {
        raw = await host.listArchive(file.path);
      } catch {
        throw fatalError('io_error', 'archive list failed');
      }
      const shape = parseWire(ArchiveListingWireSchema, raw, 'listArchive');
      return {
        entries: shape.entries.map((e) => ({ name: e.name, size: e.size, isDir: e.isDir ?? false })),
        truncated: shape.truncated,
        totalSize: shape.entries.reduce((sum, e) => sum + e.size, 0),
      };
    },
  };
}
