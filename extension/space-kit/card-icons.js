/**
 * 卡片图标：内联 SVG，不依赖页面 sprite。
 *
 * 空间卡片的 Dashboard 位于 Shadow DOM，`<use href="#id">` 无法稳定解析到
 * 文档级 sprite；这里直接写 path，保证卡片在任意挂载点都能显示。
 */

const PATHS = {
  target: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  close: '<path d="m6 6 12 12M18 6 6 18"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7"/>',
  trash: '<path d="M4 7h16M9 7V5h6v2M7 7l1 13h8l1-13"/>',
  copy: '<rect x="9" y="9" width="10" height="10" rx="2"/><path d="M15 5H5v10"/>',
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>',
  pen: '<path d="m14 5 5 5L8 21H3v-5z"/>',
  notebook: '<rect x="5" y="3" width="14" height="18" rx="2"/><path d="M9 7h6M9 11h6M9 15h4M3 7h2M3 12h2M3 17h2"/>',
  flame: '<path d="M12 3c3 3.5 5 5.8 5 9a5 5 0 0 1-10 0c0-1.8.8-3.2 2.2-4.7.3 1.4 1 2.4 1.8 2.9.1-2.4.2-4.6 1-7.2z"/>',
  gauge: '<path d="M4 17a8 8 0 1 1 16 0"/><path d="m12 13 4-4"/><circle cx="12" cy="17" r="1.2"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.4 2.6 3.6 5.6 3.6 9s-1.2 6.4-3.6 9c-2.4-2.6-3.6-5.6-3.6-9S9.6 5.6 12 3z"/>',
  help: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9a2.5 2.5 0 1 1 3.6 2.3c-.7.3-1.1.9-1.1 1.7M12 17h.01"/>',
  list: '<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
};

/** 创建一个 24×24 的内联图标；未知名称返回空 span，避免整卡崩掉。 */
export function createIcon(name, { size = 15, className = '' } = {}) {
  const paths = PATHS[name];
  if (!paths) {
    const fallback = document.createElement('span');
    fallback.className = className;
    fallback.hidden = true;
    return fallback;
  }
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', String(size));
  svg.setAttribute('height', String(size));
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '1.7');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('focusable', 'false');
  if (className) svg.setAttribute('class', className);
  svg.innerHTML = paths;
  return svg;
}

/** 图标是否已注册（测试用）。 */
export function hasIcon(name) {
  return Boolean(PATHS[name]);
}
