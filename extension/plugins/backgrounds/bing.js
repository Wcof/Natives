/**
 * Bing Daily Wallpaper Carousel Background.
 * Fetches the 8-day official Bing wallpaper archive with automatic carousel/slideshow support.
 */

import { fetchDedup, setMemCache, loadCachedBackground } from '../plugins-cache.js';

const MARKETS = [
  { code: 'zh-CN', name: '中国 (zh-CN)' },
  { code: 'en-US', name: '美国 (en-US)' },
  { code: 'ja-JP', name: '日本 (ja-JP)' },
  { code: 'en-GB', name: '英国 (en-GB)' },
  { code: 'de-DE', name: '德国 (de-DE)' },
];

let activeTimer = null;

export const bingBackground = {
  key: 'background/bing',
  name: 'Bing',
  defaultData: {
    resolution: '1920',
    mkt: 'zh-CN',
    interval: 'everyTab', // 'everyTab' | '5m' | '15m' | '1h' | 'daily'
    index: 0,
  },
  render(container, data, { t, onDataChange } = {}) {
    container.style.backgroundColor = 'transparent';
    if (activeTimer) {
      clearInterval(activeTimer);
      activeTimer = null;
    }

    const mkt = data.mkt || 'zh-CN';
    const resolution = data.resolution || '1920';
    let currentIndex = Number(data.index) || 0;

    function applyWallpaper(rawUrl) {
      if (!rawUrl) return;
      container.replaceChildren();
      container.style.backgroundColor = 'transparent';
      container.style.backgroundImage = `url("${rawUrl}")`;
      container.style.backgroundSize = 'cover';
      container.style.backgroundPosition = 'center';
      loadCachedBackground(rawUrl, { category: 'bing' }).then((url) => {
        if (url && url !== rawUrl && container.style.backgroundImage.includes(rawUrl)) {
          container.style.backgroundImage = `url("${url}")`;
        }
      }).catch(() => {});
    }

    async function loadWallpaper(idx) {
      const cacheKey = `bing_wall_${mkt}_${idx}`;
      return fetchDedup(
        cacheKey,
        async () => {
          const res = await fetch(`https://bing.biturl.top/?resolution=${resolution}&format=json&index=${idx % 8}&mkt=${encodeURIComponent(mkt)}`);
          const json = await res.json();
          return json?.url ? { url: json.url } : null;
        },
        4 * 60 * 60 * 1000,
      );
    }

    // Determine starting index based on carousel policy
    if (data.interval === 'everyTab') {
      const sessionKey = 'natives_bing_tab_idx';
      const storedIdx = Number(globalThis.sessionStorage?.getItem(sessionKey)) || 0;
      currentIndex = (storedIdx + 1) % 8;
      globalThis.sessionStorage?.setItem(sessionKey, String(currentIndex));
    }

    loadWallpaper(currentIndex)
      .then((cached) => {
        if (cached?.url) {
          applyWallpaper(cached.url);
        }
      })
      .catch(() => {
        if (!container.style.backgroundImage || container.style.backgroundImage === 'none') {
          container.style.backgroundColor = '#1a1c23';
        }
      });

    // Schedule periodic carousel if interval is set
    const intervalMap = {
      '5m': 5 * 60 * 1000,
      '15m': 15 * 60 * 1000,
      '1h': 60 * 60 * 1000,
    };
    const periodMs = intervalMap[data.interval];
    if (periodMs) {
      activeTimer = setInterval(async () => {
        currentIndex = (currentIndex + 1) % 8;
        try {
          const next = await loadWallpaper(currentIndex);
          if (next?.url) applyWallpaper(next.url);
        } catch (e) {}
      }, periodMs);
    }
  },
  renderSettings(container, data, onChange, { t } = {}) {
    const curMkt = data.mkt || 'zh-CN';
    const curInterval = data.interval || 'everyTab';
    const curRes = data.resolution || '1920';

    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field">
          <span>${t ? t('regionMarket', '壁纸地区') : '壁纸地区'}</span>
          <select id="b-mkt">
            ${MARKETS.map((m) => `<option value="${m.code}" ${m.code === curMkt ? 'selected' : ''}>${m.name}</option>`).join('')}
          </select>
        </label>
        <label class="inspector-field">
          <span>${t ? t('carouselInterval', '轮播周期') : '轮播周期'}</span>
          <select id="b-interval">
            <option value="everyTab" ${curInterval === 'everyTab' ? 'selected' : ''}>每次新标签页 (Every Tab)</option>
            <option value="5m" ${curInterval === '5m' ? 'selected' : ''}>每 5 分钟 (5 Minutes)</option>
            <option value="15m" ${curInterval === '15m' ? 'selected' : ''}>每 15 分钟 (15 Minutes)</option>
            <option value="1h" ${curInterval === '1h' ? 'selected' : ''}>每 1 小时 (1 Hour)</option>
            <option value="daily" ${curInterval === 'daily' ? 'selected' : ''}>固定当天 (Today Only)</option>
          </select>
        </label>
        <label class="inspector-field">
          <span>${t ? t('resolution', '清晰度') : '清晰度'}</span>
          <select id="b-res">
            <option value="1920" ${curRes === '1920' ? 'selected' : ''}>1080P 高清 (1920x1080)</option>
            <option value="3840" ${curRes === '3840' ? 'selected' : ''}>4K 超清 (UHD 4K)</option>
          </select>
        </label>
        <div class="inspector-actions">
          <button id="b-next-btn" class="primary" type="button">
            <svg class="icon"><use href="#i-refresh" /></svg>
            <span>${t ? t('nextWallpaper', '立即换一张壁纸') : '立即换一张壁纸'}</span>
          </button>
        </div>
      </div>
    `;

    const update = () => onChange({
      ...data,
      mkt: container.querySelector('#b-mkt').value,
      interval: container.querySelector('#b-interval').value,
      resolution: container.querySelector('#b-res').value,
    });

    container.querySelector('#b-mkt').onchange = update;
    container.querySelector('#b-interval').onchange = update;
    container.querySelector('#b-res').onchange = update;

    const nextBtn = container.querySelector('#b-next-btn');
    nextBtn.onclick = () => {
      const nextIdx = ((Number(data.index) || 0) + 1) % 8;
      onChange({ ...data, index: nextIdx });
    };
  },
  dispose() {
    if (activeTimer) {
      clearInterval(activeTimer);
      activeTimer = null;
    }
  },
};
