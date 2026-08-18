/**
 * markdown-local-images — Markdown 本地图片路径改写（fanbox fixLocalImages/cleanImgUrls 移植）
 *
 * 问题：`![](./图/封面.png)` 这类相对/本地绝对路径在 webview 里解析不到
 * （相对的是应用 origin 而非文件所在目录），Milkdown/预览中图片全裂。
 *
 * 方案：渲染前把本地图片引用改写为已授权的可加载 URL，
 * 落盘前**精确还原**为原始字符串——用改写时登记的 URL→原文映射还原，
 * 保证「打开再保存」对未动过的引用做到字节级不变（fanbox 是渲染层改写
 * 不落盘；我们在 WYSIWYG 源上改写，故必须带可逆映射）。
 * 用户新拖入的资产 URL（无原文映射）则按 assetUrlToPath 还原为真实路径。
 */

import type { PreviewContext } from '@/lib/preview/contracts';

/** 折叠路径中的 ./ 与 ../（纯字符串处理，不触文件系统） */
export function normalizePath(path: string): string {
  const isAbs = path.startsWith('/');
  const parts = path.split('/');
  const out: string[] = [];
  for (const seg of parts) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') {
      if (out.length > 0 && out[out.length - 1] !== '..') out.pop();
      else if (!isAbs) out.push('..');
      continue;
    }
    out.push(seg);
  }
  return (isAbs ? '/' : '') + out.join('/');
}

/** 判断是否应跳过改写的 URL（外链/内联数据/已是可加载 URL） */
export function isExternalUrl(url: string): boolean {
  return /^(https?:|data:|blob:|asset:|#)/i.test(url) || url.startsWith('//');
}

/** 把本地引用解析为绝对路径；非本地（外链等）返回 null */
export function resolveLocalRef(url: string, baseDir: string): string | null {
  if (!url || isExternalUrl(url)) return null;
  // http://asset.localhost/...（Windows 形态的 convertFileSrc 产物）也视为已可加载
  if (/^https?:\/\/asset\.localhost\//i.test(url)) return null;
  const unwrapped = url.startsWith('<') && url.endsWith('>') ? url.slice(1, -1) : url;
  const decoded = unwrapped.startsWith('file://') ? unwrapped.slice('file://'.length) : unwrapped;
  const abs = decoded.startsWith('/')
    ? decoded
    : normalizePath(`${baseDir.replace(/\/$/, '')}/${decoded}`);
  return normalizePath(abs);
}

/** convertFileSrc 产物 → 真实文件路径（用户新拖入的资产 URL 落盘前还原用） */
export function assetUrlToPath(url: string): string | null {
  const m = url.match(/^(?:asset:\/\/localhost|https?:\/\/asset\.localhost)(\/.*)$/i);
  if (!m) return null;
  try {
    return decodeURIComponent(m[1]!);
  } catch {
    return m[1]!;
  }
}

export interface LocalImageRewrite {
  /** 改写后的 markdown（喂给编辑器/渲染器） */
  text: string;
  /** 落盘前调用：把改写产物精确还原为原始引用，新增资产 URL 还原为真实路径 */
  restore: (md: string) => string;
}

// markdown 图片语法 ![alt](url "title") 与内联 <img src="...">
const MD_IMAGE_RE = /(!\[[^\]]*\]\()(<[^>\n]+>|[^)\s]+)((?:\s+"[^"]*")?\))/g;
const HTML_IMG_RE = /(<img\b[^>]*?\bsrc=")([^"]+)(")/gi;

/**
 * 渲染前改写：本地图片引用 → toUrl(绝对路径)。
 * @param baseDir markdown 文件所在目录（相对引用的基准）
 * @param toUrl   绝对路径 → 可加载 URL（生产传 convertFileSrc，测试可注入）
 */
export function rewriteLocalImages(
  md: string,
  baseDir: string,
  toUrl: (absPath: string) => string,
): LocalImageRewrite {
  /** 改写产物 URL → 原始引用字符串（还原时精确回填） */
  const urlToOriginal = new Map<string, string>();

  const rewriteUrl = (raw: string): string => {
    const abs = resolveLocalRef(raw, baseDir);
    if (!abs) return raw;
    const url = toUrl(abs);
    if (!url || url === raw) return raw;
    // 同一 URL 对应多个不同原文时保留首个（同文件内同图多写法极罕见，取首个已足够）
    if (!urlToOriginal.has(url)) urlToOriginal.set(url, raw);
    return url;
  };

  const text = md
    .replace(MD_IMAGE_RE, (_m, pre: string, url: string, post: string) => pre + rewriteUrl(url) + post)
    .replace(HTML_IMG_RE, (_m, pre: string, url: string, post: string) => pre + rewriteUrl(url) + post);

  const restore = (out: string): string =>
    out
      .replace(MD_IMAGE_RE, (_m, pre: string, url: string, post: string) => pre + restoreUrl(url) + post)
      .replace(HTML_IMG_RE, (_m, pre: string, url: string, post: string) => pre + restoreUrl(url) + post);

  const restoreUrl = (url: string): string => {
    const original = urlToOriginal.get(url);
    if (original !== undefined) return original;
    // 编辑期间新出现的资产 URL（如拖入图片）：还原为真实路径
    return assetUrlToPath(url) ?? url;
  };

  return { text, restore };
}

export interface LocalImageAuthorization {
  authorizeFile: PreviewContext['authorizeFile'];
  toAssetUrl: PreviewContext['toAssetUrl'];
  isWithinBase: (path: string, baseDir: string) => boolean;
  signal?: AbortSignal;
}

export interface AuthorizedLocalImageRewrite extends LocalImageRewrite {
  authorizedAssets: string[];
}

function abortIfNeeded(signal?: AbortSignal): void {
  if (!signal?.aborted) return;
  const error = new Error('local image authorization aborted');
  error.name = 'AbortError';
  throw error;
}

/** 本地图片逐个通过 PreviewContext 授权后改写；拒绝项保持原文且不生成 asset URL。 */
export async function rewriteAuthorizedLocalImages(
  md: string,
  baseDir: string,
  options: LocalImageAuthorization,
): Promise<AuthorizedLocalImageRewrite> {
  const refs = new Set<string>();
  const collect = (raw: string): void => {
    const path = resolveLocalRef(raw, baseDir);
    if (path && options.isWithinBase(path, baseDir)) refs.add(path);
  };
  for (const match of md.matchAll(MD_IMAGE_RE)) collect(match[2]!);
  for (const match of md.matchAll(HTML_IMG_RE)) collect(match[2]!);

  const urls = new Map<string, string>();
  const authorizedAssets = new Set<string>();
  for (const path of refs) {
    abortIfNeeded(options.signal);
    try {
      const file = await options.authorizeFile(path);
      abortIfNeeded(options.signal);
      if (!options.isWithinBase(file.path, baseDir)) continue;
      const url = options.toAssetUrl(file);
      if (!url) continue;
      urls.set(path, url);
      authorizedAssets.add(path);
      authorizedAssets.add(file.path);
    } catch (error) {
      if (options.signal?.aborted || (error instanceof Error && error.name === 'AbortError')) throw error;
      // 缺失/拒绝只影响该引用；保留原文交给渲染器显示为不可用资源。
    }
  }

  abortIfNeeded(options.signal);
  const rewrite = rewriteLocalImages(md, baseDir, (path) => urls.get(path) ?? '');
  return { ...rewrite, authorizedAssets: [...authorizedAssets] };
}
