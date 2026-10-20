// 独立 Web 会话管理、Bootstrap Token 交换与 CSRF 保护（ADR-0042）

import { getBaseEndpoint } from './env.js';

let currentCsrfToken = null;
let currentEpoch = null;
let sessionPromise = null;
let sessionGeneration = 0;
const sessionSignal = 'natives-web-session-revision';

if (typeof window !== 'undefined') window.addEventListener('storage', async event => {
  if (event.key !== sessionSignal) return;
  invalidateWebSession();
  const session = await initWebSession();
  window.dispatchEvent(new Event(session.authenticated ? 'natives-web-session-restored' : 'natives-web-session-required'));
});

function notifyPeerSessions() {
  // Cookie is shared, CSRF and Surface caches are document-owned. No credentials cross tabs.
  try { localStorage.setItem(sessionSignal, crypto.randomUUID()); } catch { /* Storage may be unavailable. */ }
}

export function getCsrfToken() {
  return currentCsrfToken;
}

export function getEpoch() {
  return currentEpoch;
}

export function invalidateWebSession() { sessionGeneration++; sessionPromise = null; }

export async function initWebSession() {
  if (sessionPromise) return sessionPromise;
  const generation = sessionGeneration;
  sessionPromise = (async () => {
    const base = getBaseEndpoint();

    // 1. 检查 URL Fragment 中的一次性 Bootstrap Token
    if (typeof location !== 'undefined' && location.hash) {
      const match = location.hash.match(/[#&]token=([^&]+)/);
      if (match) {
        const bootstrapToken = decodeURIComponent(match[1]);
        // 立即清除 URL 中的 Token，防止被 Referer、历史记录泄露
        history.replaceState(null, '', location.pathname + location.search);

        try {
          const res = await fetch(`${base}/web/session/exchange`, {
            signal: AbortSignal.timeout(10_000),
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({ bootstrapToken }),
          });
          if (res.ok) {
            const data = await res.json();
            if (generation !== sessionGeneration) return {authenticated:false};
            currentCsrfToken = data.csrfToken;
            currentEpoch = data.epoch;
            if (typeof sessionStorage !== 'undefined' && currentCsrfToken) {
              sessionStorage.setItem('natives_csrf', currentCsrfToken);
            }
            notifyPeerSessions();
            return { authenticated: true, csrfToken: currentCsrfToken };
          }
        } catch (e) {
          console.warn('Bootstrap token exchange failed:', e);
        }
      }
    }

    // 2. 检查现有 Cookie 会话有效性
    try {
      const res = await fetch(`${base}/web/session`, { method: 'GET', signal: AbortSignal.timeout(10_000) });
      if (res.ok) {
        const data = await res.json();
        if (generation !== sessionGeneration) return {authenticated:false};
        if (data.authenticated) {
          currentCsrfToken = data.csrfToken;
          currentEpoch = data.epoch;
          if (typeof sessionStorage !== 'undefined' && currentCsrfToken) {
            sessionStorage.setItem('natives_csrf', currentCsrfToken);
          }
          return { authenticated: true, csrfToken: currentCsrfToken };
        }
      }
    } catch (e) {
      // 服务未启动或网络故障
    }

    return { authenticated: false };
  })();

  const result = await sessionPromise;
  if (!result.authenticated && generation === sessionGeneration) sessionPromise = null;
  return result;
}

export async function exchangePairingCode(pairingCode) {
  const base = getBaseEndpoint();
  const res = await fetch(`${base}/web/session/exchange`, {
    signal: AbortSignal.timeout(10_000),
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pairingCode: pairingCode.trim() }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: 'ERR_PAIRING_FAILED' }));
    throw new Error(err.error || '配对码验证失败');
  }
  const data = await res.json();
  sessionGeneration++;
  currentCsrfToken = data.csrfToken;
  currentEpoch = data.epoch;
  sessionPromise = Promise.resolve({ authenticated: true, csrfToken: currentCsrfToken });
  if (typeof sessionStorage !== 'undefined' && currentCsrfToken) {
    sessionStorage.setItem('natives_csrf', currentCsrfToken);
  }
  if (typeof window !== 'undefined') window.dispatchEvent(new Event('natives-web-session-restored'));
  notifyPeerSessions();
  return data;
}

export async function revokeWebSession() {
  const base = getBaseEndpoint();
  if (currentCsrfToken) {
    await fetch(`${base}/web/session`, {
      method: 'DELETE',
      headers: { 'X-CSRF-Token': currentCsrfToken },
    }).catch(() => {});
  }
  if (typeof sessionStorage !== 'undefined') {
    sessionStorage.removeItem('natives_csrf');
  }
  currentCsrfToken = null;
  currentEpoch = null;
  invalidateWebSession();
  notifyPeerSessions();
}
