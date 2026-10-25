// 国际化文案与资源路径适配器（ADR-0042）

import { isExtension, getBaseEndpoint } from './env.js';
import { connectionMessages } from './connection-messages.js';

let webMessages = {};
let currentLocale = 'zh_CN';

export function getUILanguage() {
  if (isExtension && globalThis.chrome?.i18n?.getUILanguage) {
    return globalThis.chrome.i18n.getUILanguage();
  }
  if (typeof navigator !== 'undefined' && navigator.language) {
    return navigator.language.startsWith('zh') ? 'zh_CN' : 'en';
  }
  return 'zh_CN';
}

export async function initI18n(locale) {
  const nextLocale = (locale || getUILanguage()).startsWith('zh') ? 'zh_CN' : 'en';
  if (nextLocale !== currentLocale) webMessages = {};
  currentLocale = nextLocale;

  {
    try {
      const base = getBaseEndpoint();
      const res = await fetch(`${base}/_locales/${nextLocale}/messages.json`, {signal:AbortSignal.timeout(2000)});
      if (res.ok && currentLocale === nextLocale) {
        webMessages = await res.json();
      }
    } catch (_) {}
  }
}

export function getMessage(messageName, substitutions) {
  const message = webMessages[messageName]?.message || connectionMessages[currentLocale]?.[messageName];
  if(message) {
    let text=message;
    const values=Array.isArray(substitutions)?substitutions:substitutions==null?[]:[substitutions];
    values.forEach((value,index)=>{text=text.replaceAll(`$${index+1}`,String(value));});
    return text;
  }
  if (isExtension && globalThis.chrome?.i18n?.getMessage) {
    const msg = globalThis.chrome.i18n.getMessage(messageName, substitutions);
    if (msg) return msg;
  }

  return messageName;
}

export function getURL(path) {
  if (isExtension && globalThis.chrome?.runtime?.getURL) {
    return globalThis.chrome.runtime.getURL(path);
  }
  const cleanPath = path.replace(/^\//, '');
  return `${getBaseEndpoint()}/${cleanPath}`;
}

export const i18n = {
  getMessage,
  getUILanguage,
  getURL,
  initI18n,
};
