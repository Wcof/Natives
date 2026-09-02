import { escapeHtml } from '../sanitizer.js';

export const binaryTimeWidget = {
  key: 'widget/binaryTime',
  name: 'Binary Time',
  defaultData: {
    showHours: true,
    showMinutes: true,
    showSeconds: true,
    name: '',
    onColor: '#48d8b8',
    offColor: '#525252',
  },
  render(container, data = {}) {
    container.className = 'Widget BinaryTime';
    const update = () => {
      const now = new Date();
      const groups = [
        data.showHours !== false && now.getHours(),
        data.showMinutes !== false && now.getMinutes(),
        data.showSeconds !== false && now.getSeconds(),
      ].filter((value) => value !== false);
      container.replaceChildren();
      const clock = document.createElement('div');
      clock.className = 'binary-clock';
      for (const value of groups) {
        const group = document.createElement('div');
        group.className = 'binary-digit-group';
        for (const digit of String(value).padStart(2, '0')) {
          const digitEl = document.createElement('div');
          digitEl.className = 'binary-digit';
          for (const bit of [8, 4, 2, 1]) {
            const pip = document.createElement('div');
            const isOn = (Number(digit) & bit) !== 0;
            pip.className = `pip ${isOn ? 'pip--on' : ''}`;
            pip.style.backgroundColor = isOn ? (data.onColor || '#48d8b8') : (data.offColor || '#525252');
            digitEl.append(pip);
          }
          group.append(digitEl);
        }
        clock.append(group);
      }
      container.append(clock);
      if (data.name) {
        const heading = document.createElement('h2');
        heading.textContent = data.name;
        container.append(heading);
      }
    };

    update();
    const intervalId = setInterval(update, 1000);
    return () => clearInterval(intervalId);
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('name', '名称')}</span><input id="bt-name" type="text" value="${escapeHtml(data.name || '')}" /></label>
        <label class="inspector-checkbox"><input id="bt-hours" type="checkbox" ${data.showHours !== false ? 'checked' : ''} /><span>${t('showHours', '显示小时')}</span></label>
        <label class="inspector-checkbox"><input id="bt-minutes" type="checkbox" ${data.showMinutes !== false ? 'checked' : ''} /><span>${t('showMinutes', '显示分钟')}</span></label>
        <label class="inspector-checkbox"><input id="bt-seconds" type="checkbox" ${data.showSeconds !== false ? 'checked' : ''} /><span>${t('showSeconds', '显示秒')}</span></label>
        <label class="inspector-field"><span>${t('activeColor', '亮点颜色')}</span><input id="bt-on" type="color" value="${data.onColor || '#48d8b8'}" /></label>
        <label class="inspector-field"><span>${t('inactiveColor', '暗点颜色')}</span><input id="bt-off" type="color" value="${data.offColor || '#525252'}" /></label>
      </div>`;
    const update = () => onChange({
      ...data,
      name: container.querySelector('#bt-name').value,
      showHours: container.querySelector('#bt-hours').checked,
      showMinutes: container.querySelector('#bt-minutes').checked,
      showSeconds: container.querySelector('#bt-seconds').checked,
      onColor: container.querySelector('#bt-on').value,
      offColor: container.querySelector('#bt-off').value,
    });
    container.querySelectorAll('input').forEach((input) => { input.onchange = update; });
  },
  styles: `
    .BinaryTime .binary-clock { display:flex; justify-content:center; margin-bottom:1rem; }
    .BinaryTime .binary-digit-group { display:flex; margin:0 6px; }
    .BinaryTime .binary-digit { display:flex; flex-direction:column; }
    .BinaryTime .pip { width:17px; height:17px; margin:7px; border-radius:100%; transition:all .3s ease-in; }
    .BinaryTime .pip--on { transform:scale(1.1); }
    .BinaryTime h2 { text-align:center; margin:0; }
  `,
};
