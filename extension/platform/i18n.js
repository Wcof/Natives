// 国际化文案与资源路径适配器（ADR-0042）

import { isExtension, getBaseEndpoint } from './env.js';

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
  if (locale) currentLocale = locale;
  else currentLocale = getUILanguage().startsWith('zh') ? 'zh_CN' : 'en';

  {
    try {
      const base = getBaseEndpoint();
      const res = await fetch(`${base}/_locales/${currentLocale}/messages.json`);
      if (res.ok) {
        webMessages = await res.json();
      }
    } catch (_) {}
  }
}

export function getMessage(messageName, substitutions) {
  if(webMessages[messageName]?.message) {
    let text=webMessages[messageName].message;
    const values=Array.isArray(substitutions)?substitutions:substitutions==null?[]:[substitutions];
    values.forEach((value,index)=>{text=text.replaceAll(`$${index+1}`,String(value));});
    return text;
  }
  if (isExtension && globalThis.chrome?.i18n?.getMessage) {
    const msg = globalThis.chrome.i18n.getMessage(messageName, substitutions);
    if (msg) return msg;
  }

  const entry = webMessages[messageName];
  if (!entry || !entry.message) {
    return messageName;
  }

  let text = entry.message;
  if (substitutions) {
    const subs = Array.isArray(substitutions) ? substitutions : [substitutions];
    subs.forEach((sub, idx) => {
      text = text.replace(new RegExp(`\\$${idx + 1}`, 'g'), String(sub));
    });
  }
  return text;
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
