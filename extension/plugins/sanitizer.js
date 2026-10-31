

import { WIDGET_KEYS } from '../space-kit/card-meta.js';

// 卡片 key 的唯一前端来源是 space-kit/card-meta.js；这里 re-export 保持既有调用方不变。
// Rust Host 的 workspace_store/schema.rs 仍持有一份白名单，新增 key 必须同步。
export { WIDGET_KEYS };

export const BACKGROUND_KEYS = [
  'background/apod', 'background/bing', 'background/colour', 'background/giphy',
  'background/gradient', 'background/media', 'background/online', 'background/unsplash', 'background/wikimedia',
];

export const POSITIONS = [
  'topLeft', 'topCentre', 'topRight',
  'middleLeft', 'middleCentre', 'middleRight',
  'bottomLeft', 'bottomCentre', 'bottomRight',
  'free',
];

const ALLOWED_TAGS = new Set([
  'a', 'b', 'blockquote', 'br', 'code', 'div', 'em', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'hr', 'i', 'img', 'li', 'ol', 'p', 'pre', 'small', 'span', 'strong', 'sub', 'sup',
  'table', 'tbody', 'td', 'th', 'thead', 'tr', 'u', 'ul',
]);

const ALLOWED_ATTRS = new Set([
  'href', 'src', 'alt', 'title', 'class', 'target', 'rel', 'width', 'height',
]);

export function escapeHtml(str) {
  return String(str || '').replace(/[&<>"']/g, (m) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  })[m]);
}

export function sanitizeHtml(dirtyHtml) {
  const parser = new DOMParser();
  const doc = parser.parseFromString(dirtyHtml, 'text/html');
  function clean(node) {
    const children = Array.from(node.childNodes);
    for (const child of children) {
      if (child.nodeType === Node.ELEMENT_NODE) {
        const tagName = child.tagName.toLowerCase();
        if (!ALLOWED_TAGS.has(tagName)) {
          child.remove();
          continue;
        }
        for (const attr of Array.from(child.attributes)) {
          const attrName = attr.name.toLowerCase();
          if (!ALLOWED_ATTRS.has(attrName) || (attrName === 'href' && /^javascript:/i.test(attr.value))) {
            child.removeAttribute(attr.name);
          }
        }
        if (tagName === 'a') {
          child.setAttribute('rel', 'noopener noreferrer');
          child.setAttribute('target', '_blank');
        }
        clean(child);
      } else if (child.nodeType !== Node.TEXT_NODE) {
        child.remove();
      }
    }
  }
  clean(doc.body);
  return doc.body.innerHTML;
}
