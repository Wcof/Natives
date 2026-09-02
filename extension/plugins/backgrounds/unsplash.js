/**
 * Unsplash Background with Slideshow & Carousel Support.
 */

import { fetchDedup, loadCachedBackground } from '../plugins-cache.js';
import { escapeHtml } from '../sanitizer.js';

let unsplashTimer = null;

export const unsplashBackground = {
  key: 'background/unsplash',
  name: 'Unsplash',
  defaultData: { query: 'nature', accessKey: '', interval: 'everyTab' },
  render(container, data) {
    const query = data.query || 'nature';
    const key = data.accessKey || '';
    container.style.backgroundColor = 'transparent';

    if (unsplashTimer) {
      clearInterval(unsplashTimer);
      unsplashTimer = null;
    }

    function applyImage(rawUrl) {
      if (!rawUrl) return;
      container.replaceChildren();
      container.style.backgroundImage = `url("${rawUrl}")`;
      container.style.backgroundSize = 'cover';
      container.style.backgroundPosition = 'center';
      loadCachedBackground(rawUrl, { category: 'unsplash' }).then((url) => {
        if (url && url !== rawUrl && container.style.backgroundImage.includes(rawUrl)) {
          container.style.backgroundImage = `url("${url}")`;
        }
      }).catch(() => {});
    }

    if (!key) {
      applyImage('https://images.unsplash.com/photo-1506744038136-46273834b3fb?auto=format&fit=crop&w=1920&q=80');
      return;
    }

    async function loadUnsplash() {
      const sig = Date.now();
      return fetchDedup(
        `unsplash_${query}_${key}_${data.interval === 'everyTab' ? Math.floor(sig / 60000) : 'static'}`,
        async () => {
          const res = await fetch(`https://api.unsplash.com/photos/random?query=${encodeURIComponent(query)}&client_id=${encodeURIComponent(key)}`);
          const json = await res.json();
          const url = json.urls?.full || json.urls?.regular;
          return url ? { url } : null;
        },
        15 * 60 * 1000,
      );
    }

    loadUnsplash()
      .then((cached) => {
        if (cached?.url) applyImage(cached.url);
      })
      .catch(() => {
        if (!container.style.backgroundImage || container.style.backgroundImage === 'none') {
          container.style.backgroundColor = '#1e293b';
        }
      });

    const intervalMap = {
      '5m': 5 * 60 * 1000,
      '15m': 15 * 60 * 1000,
      '1h': 60 * 60 * 1000,
    };
    const periodMs = intervalMap[data.interval];
    if (periodMs) {
      unsplashTimer = setInterval(async () => {
        try {
          const next = await loadUnsplash();
          if (next?.url) applyImage(next.url);
        } catch (e) {}
      }, periodMs);
    }
  },
  renderSettings(container, data, onChange, { t } = {}) {
    const curInterval = data.interval || 'everyTab';
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t ? t('unsplashThemeQuery', '主题 (Query)') : '主题 (Query)'}</span><input type="text" id="u-query" value="${escapeHtml(data.query || 'nature')}" /></label>
        <label class="inspector-field"><span>Access Key</span><input type="text" id="u-key" value="${escapeHtml(data.accessKey || '')}" placeholder="unsplash.com API Key" /></label>
        <label class="inspector-field">
          <span>${t ? t('carouselInterval', '轮播周期') : '轮播周期'}</span>
          <select id="u-interval">
            <option value="everyTab" ${curInterval === 'everyTab' ? 'selected' : ''}>每次新标签页</option>
            <option value="5m" ${curInterval === '5m' ? 'selected' : ''}>每 5 分钟</option>
            <option value="15m" ${curInterval === '15m' ? 'selected' : ''}>每 15 分钟</option>
            <option value="1h" ${curInterval === '1h' ? 'selected' : ''}>每 1 小时</option>
            <option value="pause" ${curInterval === 'pause' ? 'selected' : ''}>固定单张</option>
          </select>
        </label>
      </div>
    `;
    const update = () => onChange({
      ...data,
      query: container.querySelector('#u-query').value.trim(),
      accessKey: container.querySelector('#u-key').value.trim(),
      interval: container.querySelector('#u-interval').value,
    });
    container.querySelector('#u-query').onchange = update;
    container.querySelector('#u-key').onchange = update;
    container.querySelector('#u-interval').onchange = update;
  },
  dispose() {
    if (unsplashTimer) {
      clearInterval(unsplashTimer);
      unsplashTimer = null;
    }
  },
};
