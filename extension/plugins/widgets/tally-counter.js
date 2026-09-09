import { escapeHtml } from '../sanitizer.js';

export const tallyCounterWidget = {
  key: 'widget/tallyCounter',
  name: 'Tally Counter',
  defaultData: { count: 0, label: '', step: 1, showReset: true },
  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key, onDataChange } = {}) {
    const count = Number(data.count) || 0;
    const step = Number(data.step) || 1;
    container.className = 'Widget TallyCounter';
    container.replaceChildren();
    if (data.label) {
      const label = document.createElement('div');
      label.className = 'label';
      label.textContent = data.label;
      container.append(label);
    }
    const countContainer = document.createElement('div');
    countContainer.className = 'count-container';
    const decrement = controlButton('minus', t('decrease', '减少'), '−', () => onDataChange?.({ ...data, count: count - step }));
    const value = document.createElement('span');
    value.className = 'count';
    value.textContent = String(count);
    const increment = controlButton('plus', t('increase', '增加'), '+', () => onDataChange?.({ ...data, count: count + step }));
    countContainer.append(decrement, value, increment);
    container.append(countContainer);
    if (data.showReset !== false) {
      const reset = document.createElement('button');
      reset.className = 'button button--primary reset-btn';
      reset.type = 'button';
      reset.textContent = t('reset', '重置');
      reset.onclick = () => onDataChange?.({ ...data, count: 0 });
      container.append(reset);
    }
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('label', '标题')}</span><input id="tc-label" type="text" value="${escapeHtml(data.label || '')}" /></label>
        <label class="inspector-field"><span>${t('currentCount', '当前值')}</span><input id="tc-count" type="number" value="${Number(data.count) || 0}" /></label>
        <label class="inspector-field"><span>${t('step', '步长')}</span><input id="tc-step" type="number" min="1" value="${Number.isFinite(Number(data.step)) ? Number(data.step) : 1}" /></label>
        <label class="inspector-checkbox"><input id="tc-reset" type="checkbox" ${data.showReset !== false ? 'checked' : ''} /><span>${t('showReset', '显示重置按钮')}</span></label>
      </div>`;
    const update = () => onChange({
      ...data,
      label: container.querySelector('#tc-label').value,
      count: Number(container.querySelector('#tc-count').value) || 0,
      step: (() => { const n = Number(container.querySelector('#tc-step').value); return Number.isFinite(n) ? Math.max(1, n) : 1; })(),
      showReset: container.querySelector('#tc-reset').checked,
    });
    container.querySelectorAll('input').forEach((input) => { input.onchange = update; });
  },
  styles: `
    .TallyCounter { display:flex; flex-direction:column; align-items:center; justify-content:center; }
    .TallyCounter .label { font-size:1.4rem; }
    .TallyCounter .count-container { display:flex; align-items:center; gap:2rem; font-weight:bold; }
    .TallyCounter .count { font-size:4rem; }
    .TallyCounter .control-btn { padding:0; width:40px; height:40px; display:flex; align-items:center; justify-content:center; }
    .TallyCounter .control-btn:active { transform:scale(.9); }
    .TallyCounter .reset-btn { font-size:.9rem; opacity:0; transition:opacity .4s; visibility:hidden; }
    .TallyCounter:hover .reset-btn { opacity:1; visibility:visible; }
  `,
};

function controlButton(kind, label, text, onClick) {
  const button = document.createElement('button');
  button.className = `button button--primary control-btn ${kind}`;
  button.type = 'button';
  button.setAttribute('aria-label', label);
  button.textContent = text;
  button.onclick = onClick;
  return button;
}
