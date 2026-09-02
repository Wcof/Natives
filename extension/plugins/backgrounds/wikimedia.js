/**
 * Wikimedia Picture of the Day Background.
 */

import { fetchDedup } from '../plugins-cache.js';

export const wikimediaBackground = {
  key: 'background/wikimedia',
  name: 'Wikimedia',
  defaultData: {},
  render(container) {
    container.style.backgroundColor = 'transparent';
    const now = new Date();
    const dateStr = now.toISOString().slice(0, 10);

    fetchDedup(
      `wikimedia_potd_${dateStr}`,
      async () => {
        const res = await fetch(`https://api.wikimedia.org/feed/v1/wikipedia/en/featured/${dateStr.replace(/-/g, '/')}`);
        const json = await res.json();
        const url = json.image?.thumbnail?.source || json.image?.image?.source || json.tfa?.thumbnail?.source;
        return url ? { url } : null;
      },
      4 * 60 * 60 * 1000,
    )
      .then((cached) => {
        if (cached?.url) {
          container.replaceChildren();
          container.style.backgroundColor = 'transparent';
          container.style.backgroundImage = `url("${cached.url}")`;
          container.style.backgroundSize = 'cover';
          container.style.backgroundPosition = 'center';
        }
      })
      .catch(() => {
        if (!container.style.backgroundImage || container.style.backgroundImage === 'none') {
          container.style.backgroundColor = '#15202b';
        }
      });
  },
  renderSettings(container, data, onChange, { t } = {}) {
    container.innerHTML = `
      <div class="inspector-notice">每日自动展示维基媒体基金会 (Wikimedia Commons) 推荐的高清精选图。</div>
    `;
  },
};
