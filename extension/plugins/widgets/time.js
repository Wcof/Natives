/**
 * Time Widget.
 * Supports digital time and analogue SVG clock (Tabliss Time.sass / Analogue.sass).
 */

export const timeWidget = {
  key: 'widget/time',
  name: 'Time',
  defaultData: {
    mode: 'digital',
    hour12: false,
    showSeconds: false,
  },
  render(container, data = {}, display = {}, { lang = 'zh_CN' } = {}) {
    container.classList.add('Time');

    function update() {
      const now = new Date();

      if (data.mode === 'analogue') {
        container.classList.add('Analogue');
        const hours = now.getHours();
        const minutes = now.getMinutes();
        const seconds = now.getSeconds();

        const hrAngle = (hours % 12) * 30 + minutes * 0.5;
        const minAngle = minutes * 6 + seconds * 0.1;
        const secAngle = seconds * 6;

        let svg = container.querySelector('svg');
        if (!svg) {
          container.replaceChildren();
          svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
          svg.setAttribute('viewBox', '0 0 100 100');
          container.append(svg);
        }

        svg.innerHTML = `
          <circle class="bezel" cx="50" cy="50" r="48" stroke="currentColor" />
          <line class="hours" x1="50" y1="50" x2="50" y2="24" stroke="currentColor" transform="rotate(${hrAngle} 50 50)" />
          <line class="minutes" x1="50" y1="50" x2="50" y2="14" stroke="currentColor" transform="rotate(${minAngle} 50 50)" />
          ${data.showSeconds ? `<line class="seconds" x1="50" y1="50" x2="50" y2="10" stroke="var(--accent, currentColor)" transform="rotate(${secAngle} 50 50)" />` : ''}
        `;
      } else {
        container.classList.remove('Analogue');
        const options = {
          hour: '2-digit',
          minute: '2-digit',
          ...(data.showSeconds ? { second: '2-digit' } : {}),
          hour12: Boolean(data.hour12),
        };
        let h1 = container.querySelector('h1');
        if (!h1) {
          container.replaceChildren();
          h1 = document.createElement('h1');
          container.append(h1);
        }
        h1.textContent = now.toLocaleTimeString(lang === 'en' ? 'en-US' : 'zh-CN', options);
      }
    }

    update();
    const intervalId = setInterval(update, data.showSeconds ? 1000 : 10000);

    return () => {
      clearInterval(intervalId);
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    const curMode = data.mode || 'digital';
    container.replaceChildren();

    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('displayMode', '显示模式')}</span>
        <select id="t-mode">
          <option value="digital" ${curMode === 'digital' ? 'selected' : ''}>${t('digitalClock', '数字时钟 (Digital)')}</option>
          <option value="analogue" ${curMode === 'analogue' ? 'selected' : ''}>${t('analogueClock', '模拟表盘 (Analogue)')}</option>
        </select>
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="h12" ${data.hour12 ? 'checked' : ''} />
        <span>${t('hour12Format', '12 小时制')}</span>
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="secs" ${data.showSeconds ? 'checked' : ''} />
        <span>${t('showSeconds', '显示秒数')}</span>
      </label>
    `;
    wrap.querySelector('#t-mode').onchange = (e) => onChange({ ...data, mode: e.target.value });
    wrap.querySelector('#h12').onchange = (e) => onChange({ ...data, hour12: e.target.checked });
    wrap.querySelector('#secs').onchange = (e) => onChange({ ...data, showSeconds: e.target.checked });
    container.append(wrap);
  },
  styles: `
    .Time { font-variant-numeric: tabular-nums; }
    .Time.Analogue { text-align: center; display: flex; justify-content: center; }
    .Time.Analogue svg { max-width: 10em; width: 100%; height: auto; }
    .Time.Analogue circle.bezel { fill: transparent; stroke-width: 2; }
    .Time.Analogue line { stroke-linecap: round; transform-origin: 50px 50px; transition: transform 0.15s cubic-bezier(0.175, 0.885, 0.32, 1.275); }
    .Time.Analogue line.hours { stroke-width: 3; }
    .Time.Analogue line.minutes { stroke-width: 2; }
    .Time.Analogue line.seconds { stroke-width: 1.5; }
  `,
};
