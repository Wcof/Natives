// 独立 Web 入口鉴权检查与配对引导装载器（ADR-0042 / F10）

import { isExtension } from './platform/env.js';
import { initWebSession, exchangePairingCode, getCsrfToken } from './platform/session.js';
import { initI18n, getUILanguage, getMessage } from './platform/i18n.js';

let pageAuthenticated = false;
if(typeof window!=='undefined' && !isExtension)window.addEventListener('natives-web-session-required',()=>renderPairingOverlay());

if (typeof window !== 'undefined') window.addEventListener('natives-web-surface', event => {
  if (event.detail?.state === 'error') showSurfaceError();
  else if (event.detail?.state === 'ready') document.getElementById('natives-surface-error')?.remove();
});

function showSurfaceError() {
  if (document.getElementById('natives-surface-error')) return;
  const notice = document.createElement('div');
  notice.id = 'natives-surface-error';
  notice.setAttribute('role', 'alert');
  notice.style.cssText = 'position:fixed;bottom:16px;left:16px;right:16px;z-index:999999;padding:14px 18px;border:1px solid var(--natives-danger);border-radius:10px;background:var(--natives-surface);color:var(--natives-text-primary);font:14px var(--natives-font);';
  notice.textContent = getUILanguage().startsWith('en')
    ? 'The local service connection was interrupted. Reload this page to reconnect.'
    : '本机服务连接已中断。请重新加载页面以恢复连接。';
  document.body.append(notice);
}

export async function ensureAuthenticated() {
  if (isExtension) {
    return true;
  }

  await initI18n();
  const session = await initWebSession();
  if (session && session.authenticated) {
    pageAuthenticated = true;
    // The platform client ensures a Surface on the first RPC; booting the page
    // must not pull the surface/session graph back into web-entry (import cycle).
    return true;
  }

  renderPairingOverlay();
  return false;
}

function renderPairingOverlay() {
  if (typeof document === 'undefined') return;
  if (document.getElementById('natives-pairing-modal')) return;

  const previouslyFocused = document.activeElement;
  const isEn = getUILanguage().startsWith('en');

  const titleText = isEn ? 'Natives Standalone Pairing' : 'Natives 本机配对';
  const descText = isEn
    ? 'No authorized browser session detected. Please run <code>natives --pair</code> in your terminal or check system diagnostics for the 10-character code:'
    : '未检测到已授权的浏览器会话。请在终端执行 <code>natives --pair</code> 或在系统诊断中获取 10 位配对码：';
  const placeholderText = '2A3B4C5D6E';
  const btnText = isEn ? 'Confirm Pairing' : '确认配对';
  const validatingText = isEn ? 'Validating...' : '验证中...';
  const closeText = isEn ? 'Close' : '关闭';

  const overlay = document.createElement('dialog');
  overlay.id = 'natives-pairing-modal';
  overlay.setAttribute('role', 'dialog');
  overlay.setAttribute('aria-modal', 'true');
  overlay.setAttribute('aria-labelledby', 'pairing-title');
  overlay.style.cssText =
    'position:fixed;inset:0;width:100vw;height:100vh;max-width:none;max-height:none;margin:0;padding:0;border:none;background:var(--natives-backdrop);backdrop-filter:blur(12px);z-index:999999;display:flex;align-items:center;justify-content:center;font-family:var(--natives-font);box-sizing:border-box;';

  overlay.innerHTML = `
    <div style="background:var(--natives-surface);color:var(--natives-text-primary);border:1px solid var(--natives-border);padding:32px;border-radius:16px;max-width:420px;width:90%;box-shadow:var(--natives-shadow);text-align:center;position:relative;box-sizing:border-box;">
      <h2 id="pairing-title" style="margin:0 0 10px 0;font-family:var(--natives-font);font-size:20px;font-weight:600;color:var(--natives-text-primary);">${titleText}</h2>
      <p style="margin:0 0 24px 0;font-size:13px;color:var(--natives-text-muted);line-height:1.6;">${descText}</p>
      <form id="pairing-form" style="display:flex;flex-direction:column;gap:16px;">
        <input id="pairing-input" type="text" maxlength="10" placeholder="${placeholderText}" autocomplete="off" autocorrect="off" autocapitalize="characters" spellcheck="false" style="padding:12px 16px;font-size:16px;letter-spacing:1px;text-align:center;border-radius:8px;border:1px solid var(--natives-border);background:var(--natives-surface-2);color:var(--natives-text-primary);outline:2px solid transparent;box-sizing:border-box;" />
        <div id="pairing-error" role="alert" style="font-size:12px;color:var(--natives-danger);display:none;"></div>
        <div style="display:flex;gap:10px;">
          <button id="pairing-btn" type="submit" style="flex:1;padding:12px;font-size:14px;font-weight:600;border-radius:8px;border:none;background:var(--natives-accent);color:var(--natives-on-accent);cursor:pointer;">${btnText}</button>
          <button id="pairing-cancel-btn" type="button" style="padding:12px 18px;font-size:14px;font-weight:500;border-radius:8px;border:1px solid var(--natives-border);background:var(--natives-surface-2);color:var(--natives-text-primary);cursor:pointer;">${closeText}</button>
        </div>
      </form>
    </div>
  `;

  document.body.appendChild(overlay);
  overlay.showModal();

  const form = overlay.querySelector('#pairing-form');
  const input = overlay.querySelector('#pairing-input');
  const errorDiv = overlay.querySelector('#pairing-error');
  const btn = overlay.querySelector('#pairing-btn');
  const cancelBtn = overlay.querySelector('#pairing-cancel-btn');

  input.focus();

  // Tab 焦点锁（Focus Trap）与 Escape 键盘支持（F10）
  const focusables = [input, btn, cancelBtn];
  const trapHandler = (e) => {
    if (!document.getElementById('natives-pairing-modal')) {
      window.removeEventListener('keydown', trapHandler, true);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      closeOverlay();
      return;
    }
    if (e.key === 'Tab') {
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const current = document.activeElement;
      if (e.shiftKey) {
        if (current === first || !overlay.contains(current)) {
          e.preventDefault();
          last.focus();
        }
      } else {
        if (current === last || !overlay.contains(current)) {
          e.preventDefault();
          first.focus();
        }
      }
    }
  };

  window.addEventListener('keydown', trapHandler, true);
  const finishPairing = () => {
    closeOverlay();
    if (!pageAuthenticated) window.location.reload();
  };
  window.addEventListener('natives-web-session-restored', finishPairing);

  function closeOverlay() {
    window.removeEventListener('keydown', trapHandler, true);
    window.removeEventListener('natives-web-session-restored', finishPairing);
    overlay.close();
    overlay.remove();
    if (previouslyFocused && typeof previouslyFocused.focus === 'function') {
      previouslyFocused.focus();
    }
  }

  cancelBtn.addEventListener('click', closeOverlay);

  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const code = input.value.trim().toUpperCase();
    if (!code) return;

    btn.disabled = true;
    btn.textContent = validatingText;
    errorDiv.style.display = 'none';

    try {
      await exchangePairingCode(code);
    } catch (err) {
      const code = String(err?.message || '');
      const key = {
        ERR_PAIRING_CODE_MISMATCH: 'webPairingMismatch',
        ERR_PAIRING_CODE_EXPIRED: 'webPairingExpired',
        ERR_PAIRING_CODE_LOCKED: 'webPairingLocked',
        ERR_PAIRING_CODE_NOT_FOUND: 'webPairingNotFound',
      }[code];
      errorDiv.textContent = (key && getMessage(key)) || err.message || (isEn ? 'Pairing failed, please retry' : '配对失败，请重试');
      errorDiv.style.display = 'block';
      btn.disabled = false;
      btn.textContent = btnText;
      input.focus();
    }
  });
}

// 自动在 Web 环境下挂载检查
if (!isExtension && typeof window !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      ensureAuthenticated();
    });
  } else {
    ensureAuthenticated();
  }
}
