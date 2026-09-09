import { fetchDedup } from '../plugins-cache.js';
import { escapeHtml } from '../sanitizer.js';
const COINGECKO_URL = 'https://api.coingecko.com/api/v3/simple/price';
const FIAT_RATES_URL = 'https://open.er-api.com/v6/latest';
const CRYPTO_IDS = new Set([
  'bitcoin', 'ethereum', 'the-open-network', 'pax-gold', 'solana',
  'dogecoin', 'cardano', 'ripple', 'litecoin', 'tether', 'binancecoin',
]);
function normalizePairs(data) {
  if (Array.isArray(data.pairs) && data.pairs.length) {
    return data.pairs.map((pair, index) => {
      const amt = Number(pair.amount);
      return {
        id: pair.id || `pair-${index}`,
        from: String(pair.from || '').toLowerCase(),
        to: String(pair.to || '').toLowerCase(),
        amount: Number.isFinite(amt) ? amt : 1,
        showChange: pair.showChange !== false,
      };
    }).filter((pair) => pair.from && pair.to);
  }
  const base = String(data.base || '').trim().toLowerCase();
  const targets = String(data.target || data.targets || '')
    .split(',').map((value) => value.trim().toLowerCase()).filter(Boolean);
  if (base && targets.length) {
    return targets.map((to, index) => ({ id: `legacy-${index}`, from: base, to, amount: 1, showChange: false }));
  }
  return null;
}
function planFetches(pairs) {
  const cryptoIds = new Set();
  const vsCurrencies = new Set();
  const fiatBases = new Set();
  for (const pair of pairs) {
    if (CRYPTO_IDS.has(pair.from)) {
      cryptoIds.add(pair.from);
      vsCurrencies.add(pair.to);
    } else {
      fiatBases.add(pair.from);
    }
  }
  return { cryptoIds: [...cryptoIds], vsCurrencies: [...vsCurrencies], fiatBases: [...fiatBases] };
}
async function fetchRates(pairs) {
  const { cryptoIds, vsCurrencies, fiatBases } = planFetches(pairs);
  const rates = {};
  await Promise.all([
    cryptoIds.length && vsCurrencies.length
      ? fetchDedup(`cg_${cryptoIds.join('_')}_${vsCurrencies.join('_')}`, async () => {
          const url = `${COINGECKO_URL}?ids=${encodeURIComponent(cryptoIds.join(','))}&vs_currencies=${encodeURIComponent(vsCurrencies.join(','))}&include_24hr_change=true`;
          const res = await fetch(url);
          if (!res.ok) throw new Error('CoinGecko request failed');
          return res.json();
        }, 15 * 60 * 1000).then((data) => {
          for (const [id, quote] of Object.entries(data || {})) {
            for (const [vs, value] of Object.entries(quote || {})) {
              if (vs.endsWith('_24h_change')) continue;
              const change = quote[`${vs}_24h_change`];
              rates[`${id}:${vs}`] = { value, change24h: typeof change === 'number' ? change : undefined };
            }
          }
        })
      : Promise.resolve(),
    ...fiatBases.map((base) => fetchDedup(`rates_${base}`, async () => {
      const res = await fetch(`${FIAT_RATES_URL}/${base.toUpperCase()}`);
      if (!res.ok) throw new Error('Exchange rate request failed');
      const data = await res.json();
      if (data.result !== 'success') throw new Error('Exchange rate request failed');
      return data.rates;
    }, 30 * 60 * 1000).then((table) => {
      for (const pair of pairs) {
        if (pair.from !== base) continue;
        const value = table?.[pair.to.toUpperCase()];
        if (value != null) rates[`${pair.from}:${pair.to}`] = { value };
      }
    })),
  ]);
  return rates;
}
export const currencyRatesWidget = {
  key: 'widget/currencyRates',
  name: 'Currency Rates',
  defaultData: {
    pairs: [
      { id: 'default-btc-usd', from: 'bitcoin', to: 'usd', showChange: true },
      { id: 'default-ton-usd', from: 'the-open-network', to: 'usd', showChange: true },
      { id: 'default-gold-usd', from: 'pax-gold', to: 'usd', showChange: true },
    ],
    decimals: 4,
  },
  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key } = {}) {
    const pairs = normalizePairs(data) || this.defaultData.pairs;
    const rawDecimals = Number(data.decimals);
    const decimals = Number.isFinite(rawDecimals) ? Math.max(0, Math.min(8, rawDecimals)) : 4;
    const formatter = new Intl.NumberFormat(undefined, { maximumFractionDigits: decimals });
    container.className = 'Widget CurrencyRates';
    container.replaceChildren();
    if (!pairs.length) {
      container.textContent = t('noPairs', '在设置中添加货币对以开始使用。');
      return () => {};
    }
    const rows = new Map();
    for (const pair of pairs) {
      const row = document.createElement('div');
      row.className = 'currency-rate-row';
      row.innerHTML = `
        <span class="currency-rate-from">${escapeHtml(String(pair.amount ?? 1))} ${escapeHtml(pair.from.toUpperCase())}</span>
        <span class="currency-rate-equals">=</span>
        <span class="currency-rate-value currency-rate-value--unavailable">—</span>
      `;
      container.append(row);
      rows.set(pair, row.querySelector('.currency-rate-value'));
    }
    let disposed = false;
    fetchRates(pairs).then((rates) => {
      if (disposed) return;
      for (const [pair, valueEl] of rows) {
        const rate = rates[`${pair.from}:${pair.to}`];
        if (!rate) continue;
        const change = pair.showChange && typeof rate.change24h === 'number'
          ? `<span class="currency-rate-change currency-rate-change--${rate.change24h >= 0 ? 'up' : 'down'}">${rate.change24h >= 0 ? '▲' : '▼'} ${Math.abs(rate.change24h).toFixed(2)}%</span>`
          : '';
        valueEl.classList.remove('currency-rate-value--unavailable');
        valueEl.innerHTML = `${escapeHtml(formatter.format(rate.value * (pair.amount ?? 1)))} ${escapeHtml(pair.to.toUpperCase())}${change}`;
      }
    }).catch(() => {
      if (disposed) return;
      for (const valueEl of rows.values()) valueEl.textContent = t('failed', '获取失败');
    });
    return () => { disposed = true; };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    const pairs = normalizePairs(data) || [];
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <div class="currency-pairs-editor">
        ${pairs.map((pair, index) => `
          <div class="currency-pair-row" data-index="${index}">
            <input type="text" class="pair-from" data-index="${index}" maxlength="40" value="${escapeHtml(pair.from)}" placeholder="bitcoin / usd" />
            <span class="currency-rate-equals">→</span>
            <input type="text" class="pair-to" data-index="${index}" maxlength="12" value="${escapeHtml(pair.to)}" placeholder="usd" />
            <input type="number" class="pair-amount" data-index="${index}" min="0" step="any" value="${escapeHtml(String(pair.amount ?? 1))}" title="Amount" />
            <button type="button" class="danger icon-button pair-del" data-index="${index}" title="${t('delete', '删除')}">×</button>
          </div>
        `).join('')}
      </div>
      <button type="button" class="primary pair-add">${t('addPair', '添加货币对')}</button>
      <label class="inspector-field"><span>${t('rateDecimals', '小数位数')}</span><input id="c-decimals" type="number" min="0" max="8" value="${escapeHtml(String(Number.isFinite(Number(data.decimals)) ? Math.max(0, Math.min(8, Number(data.decimals))) : 4))}" /></label>
    `;
    const emit = (nextPairs) => {
      const rawDec = Number(wrap.querySelector('#c-decimals')?.value);
      const nextDecimals = Number.isFinite(rawDec) ? Math.max(0, Math.min(8, rawDec)) : 4;
      onChange({
        ...data,
        pairs: nextPairs,
        decimals: nextDecimals,
      });
    };
    wrap.querySelectorAll('.pair-from, .pair-to, .pair-amount').forEach((input) => {
      input.onchange = () => {
        const index = Number(input.dataset.index);
        const next = pairs.map((pair, i) => {
          if (i !== index) return pair;
          const rawAmt = Number(wrap.querySelector(`.pair-amount[data-index="${index}"]`)?.value);
          return {
            ...pair,
            from: (wrap.querySelector(`.pair-from[data-index="${index}"]`).value || pair.from).trim().toLowerCase(),
            to: (wrap.querySelector(`.pair-to[data-index="${index}"]`).value || pair.to).trim().toLowerCase(),
            amount: Number.isFinite(rawAmt) ? rawAmt : 1,
          };
        });
        emit(next);
      };
    });
    const decimalsInput = wrap.querySelector('#c-decimals');
    if (decimalsInput) decimalsInput.onchange = () => emit(pairs);
    wrap.querySelectorAll('.pair-del').forEach((button) => {
      button.onclick = () => emit(pairs.filter((_, i) => i !== Number(button.dataset.index)));
    });
    wrap.querySelector('.pair-add').onclick = () => emit([...pairs, { id: `pair-${Date.now()}`, from: 'usd', to: 'cny', amount: 1, showChange: false }]);
    container.append(wrap);
  },
  styles: `.CurrencyRates{display:flex;flex-direction:column;gap:.4em;}.CurrencyRates .currency-rate-row{display:flex;align-items:baseline;gap:.5em;}.CurrencyRates .currency-rate-from{font-weight:bold;}.CurrencyRates .currency-rate-icon{width:1em;height:1em;vertical-align:-.15em;margin-right:.2em;}.CurrencyRates .currency-rate-equals{opacity:.6;}.CurrencyRates .currency-rate-value--unavailable{opacity:.5;}.CurrencyRates .currency-rate-change{font-size:.85em;margin-left:.25em;}.CurrencyRates .currency-rate-change--up{color:var(--success,#39d353);}.CurrencyRates .currency-rate-change--down{color:var(--danger,#ff7f8f);}.currency-pair-row{display:flex;align-items:center;gap:8px;}.currency-pair-row .pair-from{flex:1.4;min-width:0;}.currency-pair-row .pair-to{flex:1;min-width:0;}.currency-pair-row .pair-amount{width:64px;min-width:56px;}`,
};
