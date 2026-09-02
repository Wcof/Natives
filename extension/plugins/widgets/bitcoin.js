import { fetchDedup } from '../plugins-cache.js';

export const bitcoinWidget = {
  key: 'widget/bitcoin',
  name: 'Bitcoin',
  defaultData: { color: 'mempool', numberOfBlocks: 3 },
  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key } = {}) {
    container.className = 'Widget Bitcoin';
    container.replaceChildren();
    let disposed = false;
    fetchDedup('btc_blocks', async () => {
      const response = await fetch('https://mempool.space/api/v1/blocks');
      if (!response.ok) throw new Error('Mempool API error');
      return response.json();
    }, 60_000).then((blocks) => {
      if (disposed) return;
      container.replaceChildren();
      const count = Math.max(1, Math.min(5, Number(data.numberOfBlocks) || 3));
      for (const block of (Array.isArray(blocks) ? blocks : []).slice(0, count)) {
        const blockEl = document.createElement('div');
        const color = ['monochrome', 'transparent'].includes(data.color) ? data.color : 'mempool';
        blockEl.className = `bitcoin-block bitcoin-block--${color}`;
        blockEl.innerHTML = `
          <div class="block-body">
            <div class="block-height">${Number(block.height).toLocaleString()}</div>
            <div class="block-size">${formatBytes(block.size)}</div>
            <div class="transaction-count">${Number(block.tx_count).toLocaleString()} txs</div>
            <div class="time-difference">${relativeMinutes(block.timestamp, t)}</div>
          </div>`;
        blockEl.onclick = () => globalThis.open?.(`https://mempool.space/block/${encodeURIComponent(block.id || block.height)}`, '_blank', 'noopener');
        container.append(blockEl);
      }
    }).catch(() => {
      if (!disposed) container.textContent = t('failed', '获取失败');
    });
    return () => { disposed = true; };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    const color = ['mempool', 'monochrome', 'transparent'].includes(data.color) ? data.color : 'mempool';
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('color', '颜色')}</span><select id="btc-color">
          <option value="mempool" ${color === 'mempool' ? 'selected' : ''}>Mempool</option>
          <option value="monochrome" ${color === 'monochrome' ? 'selected' : ''}>${t('monochrome', '单色')}</option>
          <option value="transparent" ${color === 'transparent' ? 'selected' : ''}>${t('transparent', '透明')}</option>
        </select></label>
        <label class="inspector-field"><span>${t('numberOfBlocks', '区块数量')}</span><input id="btc-count" type="range" min="1" max="5" step="1" value="${Math.max(1, Math.min(5, Number(data.numberOfBlocks) || 3))}" /></label>
      </div>`;
    const update = () => onChange({
      ...data,
      color: container.querySelector('#btc-color').value,
      numberOfBlocks: Number(container.querySelector('#btc-count').value),
    });
    container.querySelectorAll('select,input').forEach((input) => { input.onchange = update; });
  },
  styles: `
    .Bitcoin { display:flex; justify-content:center; gap:1.5em; padding-top:.84em; padding-left:.84em; position:relative; }
    .Bitcoin .bitcoin-block { background:repeating-linear-gradient(rgb(45,51,72),rgb(45,51,72) .005575%,rgb(147,57,244) .005575%,rgb(16,95,176) 100%); cursor:pointer; width:5.24em; height:5.24em; position:relative; transform:scale(.9); }
    .Bitcoin .bitcoin-block::after { content:""; width:5.24em; height:1.196em; position:absolute; top:-1.195em; left:-1em; background:#232838; transform:skew(40deg); transform-origin:top; }
    .Bitcoin .bitcoin-block::before { content:""; width:1em; height:5.24em; position:absolute; top:-.6em; left:-.99em; background:#191c27; transform:skewY(50deg); transform-origin:top; }
    .Bitcoin .bitcoin-block--monochrome { background:#1f2432; }
    .Bitcoin .bitcoin-block--transparent { background:#bbb3; }
    .Bitcoin .bitcoin-block--transparent::after { background:#9d9d9d33; }
    .Bitcoin .bitcoin-block--transparent::before { background:#7c7c7c33; }
    .Bitcoin .block-body { display:flex; flex-direction:column; justify-content:center; align-items:center; height:100%; padding:.5em; text-align:center; box-sizing:border-box; }
    .Bitcoin .block-height { font-size:.62em; margin-bottom:.74em; }
    .Bitcoin .block-size { font-size:.75em; font-weight:bold; }
    .Bitcoin .transaction-count { font-size:.42em; margin-top:.17em; }
    .Bitcoin .time-difference { font-size:.5em; margin-top:auto; }
  `,
};

function formatBytes(bytes) {
  const size = Number(bytes) || 0;
  return size >= 1_000_000 ? `${(size / 1_000_000).toFixed(2)} MB` : `${Math.round(size / 1000)} KB`;
}

function relativeMinutes(timestamp, t) {
  const minutes = Math.max(0, Math.round((Date.now() / 1000 - Number(timestamp || 0)) / 60));
  return `${minutes} ${t('minutesAgo', 'minutes ago')}`;
}
