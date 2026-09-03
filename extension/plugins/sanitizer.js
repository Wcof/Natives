/**
 * Sanitizer and common types for Space plugins (ADR-0024 §3).
 */

export const WIDGET_KEYS = [
  'widget/binaryTime', 'widget/bookmarks', 'widget/countdown',
  'widget/css', 'widget/currencyRates', 'widget/customText', 'widget/github',
  'widget/greeting', 'widget/html', 'widget/ipInfo',
  'widget/links', 'widget/message',
  'widget/notes', 'widget/palette', 'widget/quote', 'widget/search',
  'widget/since', 'widget/tallyCounter', 'widget/time',
  'widget/todo', 'widget/topSites', 'widget/trello', 'widget/weather', 'widget/workHours',
];

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
