import { fetchDedup } from '../plugins-cache.js';
import { escapeHtml } from '../sanitizer.js';

export const currencyRatesWidget = {
  key: 'widget/currencyRates',
  name: 'Currency Rates',
  defaultData: { base: 'USD', targets: 'CNY' },
  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key } = {}) {
    const base = String(data.base || 'USD').toUpperCase();
    const targets = String(data.targets || data.target || 'CNY')
      .split(',').map((value) => value.trim().toUpperCase()).filter(Boolean);
    container.className = 'Widget CurrencyRates';
    container.replaceChildren();
    for (const target of targets) {
      const row = document.createElement('div');
      row.className = 'currency-rate-row';
      row.dataset.target = target;
      row.innerHTML = `<span class="currency-rate-from">1 ${escapeHtml(base)}</span><span class="currency-rate-equals">=</span><span class="currency-rate-value">… ${escapeHtml(target)}</span>`;
      container.append(row);
    }

    let disposed = false;
    fetchDedup(`rates_${base}`, async () => {
      const response = await fetch(`https://open.er-api.com/v6/latest/${base}`);
      if (!response.ok) throw new Error('Currency API error');
      return response.json();
    }, 30 * 60 * 1000).then((result) => {
      if (disposed) return;
      for (const row of container.querySelectorAll('.currency-rate-row')) {
        const rate = result?.rates?.[row.dataset.target];
        row.querySelector('.currency-rate-value').textContent = rate == null
          ? t('rateUnavailable', '不可用')
          : `${Number(rate).toFixed(4)} ${row.dataset.target}`;
      }
    }).catch(() => {
      if (disposed) return;
      for (const value of container.querySelectorAll('.currency-rate-value')) {
        value.textContent = t('failed', '获取失败');
      }
    });
    return () => { disposed = true; };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('baseCurrency', '基准货币')}</span><input id="c-base" type="text" maxlength="6" value="${escapeHtml(data.base || 'USD')}" /></label>
        <label class="inspector-field"><span>${t('targetCurrencies', '目标货币')}</span><input id="c-targets" type="text" value="${escapeHtml(data.targets || data.target || 'CNY')}" /></label>
      </div>`;
    const update = () => onChange({
      ...data,
      base: container.querySelector('#c-base').value.trim().toUpperCase() || 'USD',
      targets: container.querySelector('#c-targets').value.trim().toUpperCase() || 'CNY',
    });
    container.querySelectorAll('input').forEach((input) => { input.onchange = update; });
  },
  styles: `
    .CurrencyRates { display:flex; flex-direction:column; gap:.4em; }
    .CurrencyRates .currency-rate-row { display:flex; align-items:baseline; gap:.5em; }
    .CurrencyRates .currency-rate-from { font-weight:bold; }
    .CurrencyRates .currency-rate-icon { width:1em; height:1em; }
  `,
};
