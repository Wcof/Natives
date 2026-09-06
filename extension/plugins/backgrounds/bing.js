

import { fetchDedup, setMemCache, loadCachedBackground } from '../plugins-cache.js';

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (match) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[match]);
}

const MARKETS = [
  { code: 'random', name: '全球地区随机 (All Markets - 推荐)' },
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
    mkt: 'random',
    mode: 'random', // 'random' (海量历史图库) | 'recent' (最近8天归档) | 'today' (今日必应)
    interval: 'everyTab', // 'everyTab' | '5m' | '15m' | '1h' | 'daily'
    index: 0,
  },
  render(container, data = {}, { t, onDataChange } = {}) {
    container.style.backgroundColor = 'transparent';
    if (activeTimer) {
      clearInterval(activeTimer);
      activeTimer = null;
    }

    const mkt = data.mkt || 'random';
    const resolution = data.resolution || '1920';
    const mode = data.mode || 'random';
    let currentIndex = Number(data.index) || 0;

    function resolveMarket() {
      if (mkt === 'random') {
        const candidates = ['zh-CN', 'en-US', 'ja-JP', 'en-GB', 'de-DE'];
        return candidates[Math.floor(Math.random() * candidates.length)];
      }
      return mkt;
    }

    function applyWallpaper(rawUrl, meta = null) {
      if (!rawUrl) return;
      container.replaceChildren();
      container.style.backgroundColor = 'transparent';
      container.style.backgroundImage = `url("${rawUrl}")`;
      container.style.backgroundSize = 'cover';
      container.style.backgroundPosition = 'center';
      if (meta?.copyright) {
        const info = document.createElement('div');
        info.className = 'bing-wallpaper-info';
        info.innerHTML = `<span class="bing-wallpaper-title">${escapeHtml(meta.copyright)}</span>`;
        container.append(info);
      }
      loadCachedBackground(rawUrl, { category: 'bing' }).then((url) => {
        if (url && url !== rawUrl && container.style.backgroundImage.includes(rawUrl)) {
          container.style.backgroundImage = `url("${url}")`;
        }
      }).catch(() => {});
    }

    async function loadWallpaper(idx, forceFresh = false) {
      const targetMkt = resolveMarket();
      const isRandom = mode === 'random';
      const indexParam = isRandom ? 'random' : (mode === 'today' ? 0 : (idx % 8));
      const cacheKey = isRandom
        ? `bing_wall_rnd_${targetMkt}_${forceFresh ? Date.now() : (idx % 50)}`
        : `bing_wall_${targetMkt}_${indexParam}`;

      return fetchDedup(
        cacheKey,
        async () => {
          const bust = isRandom ? `&_t=${Date.now()}_${Math.random().toString(36).slice(2, 7)}` : '';
          const res = await fetch(`https://bing.biturl.top/?resolution=${resolution}&format=json&index=${indexParam}&mkt=${encodeURIComponent(targetMkt)}${bust}`);
          const json = await res.json();
          return json?.url ? { url: json.url, copyright: json.copyright } : null;
        },
        isRandom ? 2 * 60 * 1000 : 4 * 60 * 60 * 1000,
      );
    }

    // Determine starting index based on carousel policy with cross-tab persistence (localStorage)
    if (data.interval === 'everyTab') {
      const storageKey = 'natives_bing_tab_idx';
      try {
        const storedIdx = Number(globalThis.localStorage?.getItem(storageKey)) || 0;
        currentIndex = (storedIdx + 1) % 1000;
        globalThis.localStorage?.setItem(storageKey, String(currentIndex));
      } catch {
        currentIndex = (currentIndex + 1) % 1000;
      }
    }

    loadWallpaper(currentIndex)
      .then((cached) => {
        if (cached?.url) {
          applyWallpaper(cached.url, cached);
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
        currentIndex = (currentIndex + 1) % 1000;
        try {
          const next = await loadWallpaper(currentIndex, true);
          if (next?.url) applyWallpaper(next.url, next);
        } catch (e) {}
      }, periodMs);
    }
  },
  renderSettings(container, data = {}, onChange, { t } = {}) {
    const curMkt = data.mkt || 'random';
    const curMode = data.mode || 'random';
    const curInterval = data.interval || 'everyTab';
    const curRes = data.resolution || '1920';

    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field">
          <span>${t ? t('bingMode', '壁纸内容库') : '壁纸内容库'}</span>
          <select id="b-mode">
            <option value="random" ${curMode === 'random' ? 'selected' : ''}>海量历史精选图库 (成千上万张，每次不重样 - 推荐)</option>
            <option value="recent" ${curMode === 'recent' ? 'selected' : ''}>最近 8 天官方归档轮播 (Past 8 Days)</option>
            <option value="today" ${curMode === 'today' ? 'selected' : ''}>固定今日官方主推 (Today Only)</option>
          </select>
        </label>
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
            <option value="daily" ${curInterval === 'daily' ? 'selected' : ''}>仅在每日更新 (Daily)</option>
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
      mode: container.querySelector('#b-mode').value,
      mkt: container.querySelector('#b-mkt').value,
      interval: container.querySelector('#b-interval').value,
      resolution: container.querySelector('#b-res').value,
    });

    container.querySelector('#b-mode').onchange = update;
    container.querySelector('#b-mkt').onchange = update;
    container.querySelector('#b-interval').onchange = update;
    container.querySelector('#b-res').onchange = update;

    const nextBtn = container.querySelector('#b-next-btn');
    nextBtn.onclick = () => {
      const nextIdx = ((Number(data.index) || 0) + 1) % 1000;
      onChange({ ...data, index: nextIdx, _refresh: Date.now() });
    };
  },
  dispose() {
    if (activeTimer) {
      clearInterval(activeTimer);
      activeTimer = null;
    }
  },
};


