// T12 · Markdown renderer URL policy
//
// 安全模型（SEC-001）：file markdown 的本地图片 asset URL 只能在 provider 内、
// 经 PreviewContext.authorizeFile **逐资源授权**后生成（见 rewriteAuthorizedLocalImages）。
// 本文件是纯策略层：只负责「已授权 asset URL 放行 / 其余走 markdown-safety 判定」，
// 不再自行把相对路径改写成 asset://localhost URL——renderer 无 ctx，无权生成资产 URL。
// 所有 policy 均基于 markdown-safety 的既有安全判定，禁止新增宽松规则。

import { isSafeImageSource, isSafeMarkdownUrl } from '@/lib/markdown-safety';
import {
  rewriteAuthorizedLocalImages as rewriteAuthorizedLocalImagesWithContext,
} from '@/lib/markdown-local-images';
import type { PreviewContext, PreviewUrlPolicy } from '../contracts';

/** 已授权本地资产 URL（convertFileSrc 产物）：asset://localhost 或 http(s)://asset.localhost */
export function isAuthorizedAssetUrl(url: string): boolean {
  const value = String(url ?? '').trim();
  return value.startsWith('asset://localhost/') || /^https?:\/\/asset\.localhost\//i.test(value);
}

/**
 * P0-006: 只有落在授权根（baseDir）内的本地路径才能生成 asset URL。
 * markdown 里 `![](/etc/passwd)` 这类越根绝对路径必须被拒——
 * 不生成 asset:// URL（保留原文，随后被 urlTransform 的 markdown-safety 判定拦掉），
 * 而不是把任意进程可读路径拼进资产链。
 */
export function isWithinAuthorizedBase(absPath: string, baseDir: string): boolean {
  if (!baseDir) return false;
  const base = baseDir.replace(/\/+$/, '');
  return absPath === base || absPath.startsWith(base + '/');
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
  /** 渲染前对 source 的改写（file markdown 本地图片 → 可加载 URL）；无改写返回 undefined */
  rewrite?: (source: string) => string;
  urlTransform: (url: string, key?: string, node?: { tagName?: string } | null) => string;
}

/** 按 urlPolicy 构建渲染选项；baseDir 仅 file markdown（authorized-file-assets）使用 */
export function buildMarkdownRenderOptions(
  urlPolicy: PreviewUrlPolicy,
  _baseDir?: string,
): MarkdownRenderOptions {
  // SEC-001: authorized-file-assets 不再在 renderer 侧改写——相对路径 → asset URL
  // 的改写必须在 provider 里经 authorizeFile 逐资源授权后完成（见
  // rewriteAuthorizedLocalImages）。renderer 只做 URL 策略（放行已授权 asset URL，
  // 拦截其余），未授权引用保持原文 → markdown-safety 判定，不产出可访问 URL。
  if (urlPolicy === 'authorized-file-assets') {
    return { urlTransform: transformAuthorizedFileUrl };
  }
  return { urlTransform: transformAssistantSafeUrl };
}

// ── 逐资源授权改写（SEC-001）───────────────────────────────────────────
// 在 provider（async、持有 PreviewContext）内调用：先扫描 markdown 中的本地
// 图片引用，逐个经 ctx.authorizeFile 授权；仅授权通过者改写为 asset:// URL。
// 未授权/缺失/越根引用保持原文，由 urlTransform 的 markdown-safety 判定拦掉，
// 绝不产出可访问的 asset:// URL，也不让单张坏图拖垮整篇文档预览。

/**
 * 逐资源授权并改写：返回渲染源与已授权资产清单。
 * 仅在 authorized-file-assets（file markdown）路径使用；assistant-safe 不走这里。
 */
export async function rewriteAuthorizedLocalImages(
  source: string,
  baseDir: string,
  authorizeFile: PreviewContext['authorizeFile'],
): Promise<{ text: string; authorizedAssets: string[] }> {
  const { text, authorizedAssets } = await rewriteAuthorizedLocalImagesWithContext(source, baseDir, {
    authorizeFile,
    toAssetUrl: (file) => `asset://localhost${encodeURI(file.path)}`,
    isWithinBase: isWithinAuthorizedBase,
  });
  return { text, authorizedAssets };
}
