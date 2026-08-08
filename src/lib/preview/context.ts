// PreviewContext 默认实现（T10 · C0 frozen）
//
// PreviewContext 不是第二个 Host Facade：它是 provider 完成「只读准备」所需的
// 最小能力注入。默认实现组合现有文件域入口（files-api），并按 T21 约定预留
// htmlPreviewApi 接入点。file source 必须先 authorizeFile 才能 read/asset/html/list。

import type { ArchiveEntry, FileKind, ReadFileResult } from '@/types/file';
import { archiveApi, fsApi } from '@/lib/files-api';
import type { AuthorizedPreviewFile, HtmlPreviewPrepared, PreviewContext } from './contracts';
import { fatalError } from './errors';

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

/** context 依赖的 Host 能力（窄接口，便于测试注入与 T21 统一接线） */
export interface PreviewHost {
  stat(path: string): Promise<PreviewStatShape>;
  readFile(path: string): Promise<unknown>;
  listArchive(path: string): Promise<PreviewArchiveShape>;
  toAssetUrl(path: string): string;
  prepareHtml(path: string): Promise<HtmlPreviewPrepared>;
}

/** 生产默认 host：走 files-api 唯一入口；htmlPreview 属顶层 nativesAPI（T21 收敛到 files-api） */
export function defaultPreviewHost(): PreviewHost {
  const natives = typeof window !== 'undefined' ? (window as unknown as { nativesAPI?: { htmlPreview?: { prepare(path: string): Promise<HtmlPreviewPrepared> } } }).nativesAPI : undefined;
  return {
    stat: (path) => fsApi().stat(path),
    readFile: (path) => fsApi().readFile(path),
    listArchive: (path) => archiveApi().list(path),
    // 只有已授权文件会到达这里（PreviewContext.toAssetUrl 只在 authorize 后暴露）
    toAssetUrl: (path) => fsApi().convertFileSrc(path),
    prepareHtml: (path) => {
      if (!natives?.htmlPreview) throw fatalError('host_error', 'htmlPreview API not available');
      return natives.htmlPreview.prepare(path);
    },
  };
}

function asReadFileResult(value: unknown): ReadFileResult {
  return value as ReadFileResult;
}

function asArchiveListing(shape: PreviewArchiveShape): { entries: ArchiveEntry[]; truncated: boolean; totalSize: number } {
  return {
    entries: shape.entries.map((e) => ({ name: e.name, size: e.size, isDir: e.isDir ?? false })),
    truncated: shape.truncated,
    totalSize: shape.entries.reduce((sum, e) => sum + (e.size ?? 0), 0),
  };
}

export function createPreviewContext(host: PreviewHost = defaultPreviewHost()): PreviewContext {
  return {
    /** 授权：Host stat → found && !isDir → AuthorizedPreviewFile；否则致命，绝不静默降级 */
    async authorizeFile(path: string): Promise<AuthorizedPreviewFile> {
      let stat: PreviewStatShape;
      try {
        stat = await host.stat(path);
      } catch (error) {
        throw fatalError('io_error', `stat failed for ${path}: ${String(error)}`);
      }
      if (!stat.found) {
        throw fatalError('permission_denied', `path not accessible: ${path}`);
      }
      if (stat.isDir) {
        throw fatalError('security_violation', `directory cannot be previewed as file: ${path}`);
      }
      return {
        path: stat.path ?? path,
        name: stat.name ?? path.split('/').pop() ?? path,
        kind: (stat.kind as FileKind) ?? 'other',
        size: stat.size ?? 0,
        mtime: stat.mtime ?? 0,
      };
    },

    async readText(file: AuthorizedPreviewFile): Promise<ReadFileResult> {
      try {
        return asReadFileResult(await host.readFile(file.path));
      } catch (error) {
        throw fatalError('io_error', `read failed for ${file.path}: ${String(error)}`);
      }
    },

    toAssetUrl(file: AuthorizedPreviewFile): string {
      // 只有已授权文件允许生成 asset URL；禁止 raw path -> convertFileSrc
      return host.toAssetUrl(file.path);
    },

    async prepareHtml(file: AuthorizedPreviewFile): Promise<HtmlPreviewPrepared> {
      try {
        return await host.prepareHtml(file.path);
      } catch (error) {
        if (error instanceof Error && error.name === 'PreviewProviderError') throw error;
        throw fatalError('host_error', `html prepare failed for ${file.path}: ${String(error)}`);
      }
    },

    async listArchive(file: AuthorizedPreviewFile): Promise<{ entries: ArchiveEntry[]; truncated: boolean; totalSize: number }> {
      try {
        return asArchiveListing(await host.listArchive(file.path));
      } catch (error) {
        throw fatalError('io_error', `archive list failed for ${file.path}: ${String(error)}`);
      }
    },
  };
}
