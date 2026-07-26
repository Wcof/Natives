/**
 * Pure safety helpers for assistant Markdown (no CSS / React).
 * Shared by MarkdownText.tsx and unit tests under `tsx --test`.
 */

/** GFM-friendly allowlist — no script/style/iframe/form hosts. */
export const SAFE_MARKDOWN_ELEMENTS = [
  'a',
  'blockquote',
  'br',
  'code',
  'del',
  'em',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'hr',
  'img',
  'input',
  'li',
  'ol',
  'p',
  'pre',
  'strong',
  'table',
  'tbody',
  'td',
  'th',
  'thead',
  'tr',
  'ul',
] as const;

const EVENT_ATTR = /^on[a-z]+$/i;

/**
 * Allow only http(s), mailto, in-page anchors, and safe relative paths.
 * Rejects javascript:, data:, file:, vbscript:, etc.
 */
export function isSafeMarkdownUrl(raw: string): boolean {
  const value = String(raw ?? '').trim();
  if (!value) return false;

  if (value.startsWith('#')) {
    return !/[\s<>"'`]|javascript:/i.test(value.slice(1));
  }

  if (value.startsWith('//')) return false;

  const schemeMatch = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(value);
  if (schemeMatch) {
    const scheme = schemeMatch[1]!.toLowerCase();
    return scheme === 'http' || scheme === 'https' || scheme === 'mailto';
  }

  const lowered = value.toLowerCase();
  if (
    lowered.includes('javascript:') ||
    lowered.includes('data:') ||
    lowered.includes('file:') ||
    lowered.includes('vbscript:')
  ) {
    return false;
  }
  return true;
}

/**
 * Inline image payloads: only raster data URLs with strict mime + base64 body.
 * Applies to <img src> ONLY — every other tag/attribute keeps rejecting data:.
 */
const SAFE_IMAGE_DATA_URL = /^data:image\/(?:png|jpe?g|gif|webp);base64,[a-zA-Z0-9+/=]+$/;

export function isSafeImageSource(raw: string): boolean {
  const value = String(raw ?? '').trim();
  if (SAFE_IMAGE_DATA_URL.test(value)) return true;
  return isSafeMarkdownUrl(value);
}

/**
 * react-markdown urlTransform: unsafe → empty string (dropped).
 * react-markdown calls this as (url, key, node); when the target is an
 * <img src>, base64 raster data URLs are additionally allowed.
 */
export function transformMarkdownUrl(
  url: string,
  key?: string,
  node?: { tagName?: string } | null,
): string {
  if (key === 'src' && node?.tagName?.toLowerCase() === 'img') {
    return isSafeImageSource(url) ? url : '';
  }
  return isSafeMarkdownUrl(url) ? url : '';
}

function stripDangerousProps(props: Record<string, unknown> | null | undefined): void {
  if (!props) return;
  for (const key of Object.keys(props)) {
    if (EVENT_ATTR.test(key) || key === 'style' || key === 'srcdoc') {
      delete props[key];
    }
  }
  if (typeof props.className === 'string' && /expression\s*\(/i.test(props.className)) {
    delete props.className;
  }
}

/**
 * rehype-rewrite visitor: drop dangerous tags leftover and event/style attrs.
 * Compatible with rehype-rewrite's (node, index, parent) signature.
 */
export function rewriteMarkdownNode(node: unknown): void {
  if (!node || typeof node !== 'object') return;
  const n = node as {
    type?: string;
    tagName?: string;
    properties?: Record<string, unknown>;
    children?: unknown[];
  };
  if (n.type !== 'element' || !n.tagName) return;

  const tag = n.tagName.toLowerCase();
  if (
    tag === 'script' ||
    tag === 'style' ||
    tag === 'iframe' ||
    tag === 'object' ||
    tag === 'embed' ||
    tag === 'form' ||
    tag === 'link' ||
    tag === 'meta' ||
    tag === 'base'
  ) {
    (n as { type: string; value?: string }).type = 'text';
    (n as { value?: string }).value = '';
    delete n.tagName;
    delete n.properties;
    delete n.children;
    return;
  }

  stripDangerousProps(n.properties);

  if (tag === 'a' && n.properties) {
    const href = n.properties.href;
    if (typeof href === 'string' && !isSafeMarkdownUrl(href)) {
      n.properties.href = '';
    }
    if (typeof n.properties.href === 'string' && /^https?:/i.test(n.properties.href)) {
      n.properties.target = '_blank';
      n.properties.rel = 'noopener noreferrer nofollow';
    }
  }

  if (tag === 'img' && n.properties) {
    const src = n.properties.src;
    if (typeof src === 'string' && !isSafeImageSource(src)) {
      n.properties.src = '';
    }
  }

  if (tag === 'table') {
    // Wide GFM tables must scroll inside their own box instead of blowing the
    // conversation column open. A dedicated class keeps the CSS hook explicit
    // (styled in MarkdownText.module.css) without widening the element allowlist.
    n.properties = n.properties ?? {};
    const existing = n.properties.className;
    const classes = Array.isArray(existing)
      ? existing.map(String)
      : typeof existing === 'string' && existing
        ? existing.split(/\s+/)
        : [];
    if (!classes.includes('md-table-overflow')) classes.push('md-table-overflow');
    n.properties.className = classes;
  }

  if (tag === 'input' && n.properties) {
    n.properties.type = 'checkbox';
    n.properties.disabled = true;
  }
}

export function pluginsFilter(
  type: 'rehype' | 'remark',
  plugins: unknown[],
): unknown[] {
  if (type !== 'rehype') return plugins;
  return plugins.filter((plugin) => {
    if (!plugin) return false;
    const candidate = Array.isArray(plugin) ? plugin[0] : plugin;
    const name =
      (candidate &&
        typeof candidate === 'object' &&
        'name' in candidate &&
        String((candidate as { name?: string }).name)) ||
      (typeof candidate === 'function' && candidate.name) ||
      String(candidate);
    return !/rehype-?raw/i.test(name);
  });
}
