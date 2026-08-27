/**
 * apps-web-url — Web 应用注册/编辑表单的 URL 规范化（APPV2-T01）。
 *
 * 与 Rust 侧 `src-tauri/src/apps/web_url.rs` 同策略（后端为最终权威，前端只做
 * 提交前规范化与内联校验，避免把明显非法输入打到后端）：
 * - 无 scheme 输入补 `https://`；
 * - 仅接受 http/https；公网必须 https；显式 http 仅 loopback；
 * - `deriveOriginFromUrl` 从 URL 推导默认 approved origin（host）。
 *
 * 注意：本 helper 不是后端校验的替代——后端 `normalize_web_url` 对同一输入
 * 给出同样的规范化结果，双端不一致时以后端为准。
 */

export interface WebUrlCheck {
  ok: boolean;
  /** 规范化后的 URL（ok=false 时为 null）。 */
  normalized: string | null;
  /** 规范化结果与原始输入不同（用于表单展示“将保存为 …”）。 */
  changed: boolean;
  reason: 'empty' | 'scheme' | 'public-http' | 'invalid-host' | null;
}

function isLoopbackHost(host: string): boolean {
  return host === '127.0.0.1' || host === 'localhost' || host === '::1';
}

function hostOf(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  const rest = trimmed.includes('://') ? trimmed.split('://')[1] ?? '' : trimmed;
  const authority = (rest.split(/[/?#]/)[0] ?? '').trim();
  if (!authority) return null;
  const noUser = authority.includes('@') ? authority.slice(authority.lastIndexOf('@') + 1) : authority;
  let host = noUser;
  if (noUser.startsWith('[')) {
    host = noUser.slice(1, noUser.indexOf(']') >= 0 ? noUser.indexOf(']') : noUser.length);
  } else if (noUser.includes(':')) {
    host = noUser.slice(0, noUser.lastIndexOf(':'));
  }
  const lower = host.toLowerCase();
  return lower ? lower : null;
}

/** 规范化 + 校验用户输入的 Web URL（纯函数，可测试）。 */
export function checkWebUrl(input: string): WebUrlCheck {
  const trimmed = input.trim();
  if (!trimmed) return { ok: false, normalized: null, changed: false, reason: 'empty' };

  const candidate = trimmed.includes('://') ? trimmed : `https://${trimmed}`;
  const schemeMatch = candidate.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):\/\//);
  if (!schemeMatch) return { ok: false, normalized: null, changed: candidate !== trimmed, reason: 'invalid-host' };

  const scheme = (schemeMatch[1] ?? '').toLowerCase();
  if (scheme !== 'http' && scheme !== 'https') {
    return { ok: false, normalized: null, changed: candidate !== trimmed, reason: 'scheme' };
  }
  const host = hostOf(candidate);
  if (!host) return { ok: false, normalized: null, changed: candidate !== trimmed, reason: 'invalid-host' };
  if (!isLoopbackHost(host) && (!host.includes('.') || host.startsWith('.') || host.endsWith('.') || host.includes('..'))) {
    return { ok: false, normalized: null, changed: candidate !== trimmed, reason: 'invalid-host' };
  }
  if (scheme === 'http' && !isLoopbackHost(host)) {
    return { ok: false, normalized: null, changed: candidate !== trimmed, reason: 'public-http' };
  }
  return { ok: true, normalized: candidate, changed: candidate !== trimmed, reason: null };
}

/** 从 URL 推导默认 approved origin（host；解析失败返回 null）。 */
export function deriveOriginFromUrl(input: string): string | null {
  return hostOf(input);
}
