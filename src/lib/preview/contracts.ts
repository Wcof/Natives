// Preview Capability V2 核心契约（T10 · C0 frozen）
//
// 本文件是 Files / Assistant / Artifact / Follow 共用「只读预览」的契约唯一权威。
// 变更需 T10 单写入者；T19 负责 composition 组装；Leaf provider/renderer 只消费本契约。
// 禁止在此引入第二套 FS/Session 权威：增删改、editor dirty、follow 状态仍由现有域负责。

import type { ArchiveEntry, FileKind, ReadFileResult, ArchiveListing } from '@/types/file';

/** 预览 Surface 标识：每个 Surface 持有自己的 PreviewRequestController */
export type PreviewSurfaceId = 'files' | 'assistant' | 'artifact' | 'follow';

/** 右侧面板预览子模式(T213:从 shell 域下沉到共享契约,消除 files↔shell 交叉依赖 R-E3) */
export type PreviewSubMode = 'preview' | 'info' | 'git';

/** 预览输入：文件或内存内容。第一版就支持 memory，避免 Assistant 另起一条渲染线 */
export type PreviewSource =
  | {
      type: 'file';
      path: string;
      name?: string;
      kind?: FileKind;
      size?: number;
      mtime?: number;
    }
  | {
      type: 'memory';
      name: string;
      content: string;
      mime?: string;
    };

export interface PreviewRequest {
  source: PreviewSource;
  /** 本轮只做只读预览；编辑走现有 Editor 路径 */
  mode: 'preview';
  surface: PreviewSurfaceId;
  /** 由 Surface 的 PreviewRequestController.next() 提供 */
  signal?: AbortSignal;
}

/** 已授权的文件引用：任何 file source 在 read/asset/html 前必须经 PreviewContext.authorizeFile */
export interface AuthorizedPreviewFile {
  path: string;
  name: string;
  kind: FileKind;
  size: number;
  mtime: number;
}

/** Markdown URL policy：普通 Assistant 用严格默认；文件 Markdown 保留已授权本地图 */
export type PreviewUrlPolicy = 'assistant-safe' | 'authorized-file-assets';

/** HTML Host prepare 结果（wire 形态随 T20 冻结后由 T21 更新） */
export interface HtmlPreviewPrepared {
  content: string;
  fsBase: string;
  serverPort: number;
}

/** 类型化预览模型：renderer 只消费它，不决定「这个 path 该用谁」 */
export type PreviewModel =
  | {
      kind: 'markdown';
      source: string;
      truncated: boolean;
      baseDir?: string;
      urlPolicy: PreviewUrlPolicy;
    }
  | { kind: 'html'; revision: string; previewUrl?: string; html?: string; sandbox: string }
  | { kind: 'json'; value: unknown; formatted: string; nodeCount: number; truncated: boolean }
  | { kind: 'code'; source: string; language: string; truncated: boolean }
  | { kind: 'image'; src: string; name: string }
  | { kind: 'video'; src: string; name: string }
  | { kind: 'audio'; src: string; name: string }
  | { kind: 'pdf'; src: string; name: string }
  | { kind: 'csv'; headers: string[]; rows: string[][]; truncated: boolean }
  | { kind: 'archive'; entries: ArchiveEntry[]; truncated: boolean }
  | { kind: 'unsupported'; reason: string };

/**
 * PreviewContext：provider 读取所需的最小只读 DI。
 * 不是第二个 Host Facade——默认实现组合现有 fsApi/archiveApi/htmlPreviewApi。
 * file source 必须先 authorizeFile 才能 readText/toAssetUrl/prepareHtml/listArchive。
 */
export interface PreviewContext {
  authorizeFile(path: string): Promise<AuthorizedPreviewFile>;
  readText(file: AuthorizedPreviewFile): Promise<ReadFileResult>;
  toAssetUrl(file: AuthorizedPreviewFile): string;
  prepareHtml(file: AuthorizedPreviewFile): Promise<HtmlPreviewPrepared>;
  listArchive(file: AuthorizedPreviewFile): Promise<ArchiveListing>;
}

export interface PreviewProvider {
  id: string;
  priority: number;
  accepts(request: PreviewRequest): boolean;
  prepare(request: PreviewRequest, ctx: PreviewContext): Promise<PreviewModel>;
}

export type ProviderLoader = () => Promise<PreviewProvider>;

/** T30/T31 冻结接缝（C0）：虚拟文件视图句柄。T30 实现，T31 持有真实 scroll container 并调用 */
export interface VirtualFileViewHandle {
  scrollToIndex(
    index: number,
    options?: { align?: 'auto' | 'start' | 'center' | 'end' },
  ): void;
  getColumnCount(): number;
}
