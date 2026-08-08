// T12 · Markdown renderer URL policy
//
// 普通 Assistant Markdown 用默认严格 policy（SafeMarkdown 默认 urlTransform）。
// 文件 Markdown（authorized-file-assets）必须：
//   1) 保留本地图片 rewrite（rewriteLocalImages，toUrl → asset://localhost…）；
//   2) 允许已授权 asset://localhost / http(s)://asset.localhost 的 URL，
//      不得用默认 SafeMarkdown policy 把已授权本地图再次拦掉（R23）。
// 所有 policy 均基于 markdown-safety 的既有安全判定，禁止新增宽松规则。

import { isSafeImageSource, isSafeMarkdownUrl } from '@/lib/markdown-safety';
import { rewriteLocalImages } from '@/lib/markdown-local-images';
import type { PreviewUrlPolicy } from '../contracts';

/** 已授权本地资产 URL（convertFileSrc 产物）：asset://localhost 或 http(s)://asset.localhost */
export function isAuthorizedAssetUrl(url: string): boolean {
  const value = String(url ?? '').trim();
  return value.startsWith('asset://localhost/') || /^https?:\/\/asset\.localhost\//i.test(value);
}

/**
 * file markdown 的 urlTransform：已授权资产 URL 直接放行，其余走 markdown-safety 判定。
 * 签名与 markdown-safety.transformMarkdownUrl 对齐（react-markdown 兼容）。
 */
export function transformAuthorizedFileUrl(
  url: string,
  key?: string,
  node?: { tagName?: string } | null,
): string {
  if (isAuthorizedAssetUrl(url)) return url;
  if (key === 'src' && node?.tagName?.toLowerCase() === 'img') {
    return isSafeImageSource(url) ? url : '';
  }
  return isSafeMarkdownUrl(url) ? url : '';
}

/** 默认严格 policy（assistant-safe）：等价于 SafeMarkdown 默认行为 */
export function transformAssistantSafeUrl(
  url: string,
  key?: string,
  node?: { tagName?: string } | null,
): string {
  if (key === 'src' && node?.tagName?.toLowerCase() === 'img') {
    return isSafeImageSource(url) ? url : '';
  }
  return isSafeMarkdownUrl(url) ? url : '';
}

export interface MarkdownRenderOptions {
  /** 渲染前对 source 的改写（文件 markdown 本地图片 → 可加载 URL）；无改写返回 undefined */
  rewrite?: (source: string) => string;
  urlTransform: (url: string, key?: string, node?: { tagName?: string } | null) => string;
}

/** 按 urlPolicy 构建渲染选项；baseDir 仅 file markdown（authorized-file-assets）使用 */
export function buildMarkdownRenderOptions(
  urlPolicy: PreviewUrlPolicy,
  baseDir?: string,
): MarkdownRenderOptions {
  if (urlPolicy === 'authorized-file-assets') {
    return {
      rewrite: (source: string): string =>
        rewriteLocalImages(source, baseDir ?? '', (abs) => `asset://localhost${encodeURI(abs)}`).text,
      urlTransform: transformAuthorizedFileUrl,
    };
  }
  return { urlTransform: transformAssistantSafeUrl };
}
