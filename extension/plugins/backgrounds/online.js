/**
 * Online Image URL Background.
 */

import { escapeHtml } from '../sanitizer.js';

export const onlineBackground = {
  key: 'background/online',
  name: 'Online URL',
  defaultData: { url: '' },
  render(container, data) {
    container.replaceChildren();
    if (data.url && /^https?:\/\//i.test(data.url)) {
      container.style.backgroundColor = 'transparent';
      container.style.backgroundImage = `url("${data.url.replace(/"/g, '\\"')}")`;
      container.style.backgroundSize = 'cover';
      container.style.backgroundPosition = 'center';
    } else {
      container.style.backgroundColor = '#101010';
      container.style.backgroundImage = 'none';
    }
  },
  renderSettings(container, data, onChange) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>图片 URL</span><input type="url" value="${escapeHtml(data.url || '')}" placeholder="https://..." /></label>
      </div>
    `;
    container.querySelector('input').onchange = (e) => onChange({ ...data, url: e.target.value.trim() });
  },
};
