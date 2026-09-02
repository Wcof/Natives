/**
 * Colour Palette Widget.
 * Calibrated TablissNG Palette.sass with 500x200 canvas, color strip expansion, rotated hex labels, and shimmer.
 */

const SAMPLE_PALETTES = [
  ['#264653', '#2a9d8f', '#e9c46a', '#f4a261', '#e76f51'],
  ['#0d1b2a', '#1b263b', '#415a77', '#778da9', '#e0e1dd'],
  ['#2b2d42', '#8d99ae', '#edf2f4', '#ef233c', '#d90429'],
  ['#606c38', '#283618', '#fefae0', '#dda15e', '#bc6c25'],
  ['#003049', '#d62828', '#f77f00', '#fcbf49', '#eae2b6'],
  ['#10002b', '#240046', '#3c096c', '#5a189a', '#7b2cbf'],
  ['#03071e', '#370617', '#6a040f', '#9d0208', '#dc2f02'],
];

export const paletteWidget = {
  key: 'widget/palette',
  name: 'Colour Palette',
  defaultData: {
    paletteIndex: 0,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k, onDataChange } = {}) {
    const idx = Number(data.paletteIndex) || 0;
    const colors = SAMPLE_PALETTES[idx % SAMPLE_PALETTES.length];

    container.className = 'Widget Palette';
    container.replaceChildren();

    let resetTimer = null;

    colors.forEach((col) => {
      const colorStrip = document.createElement('div');
      colorStrip.className = 'Color';
      colorStrip.style.backgroundColor = col;
      colorStrip.title = `${col} (${t('clickToCopy', '点击复制')})`;

      const label = document.createElement('span');
      label.className = 'label';
      label.textContent = col;
      colorStrip.append(label);

      colorStrip.onclick = (e) => {
        e.stopPropagation();
        if (navigator.clipboard?.writeText) {
          navigator.clipboard.writeText(col).catch(() => {});
        }
        label.textContent = t('copied', '已复制');
        clearTimeout(resetTimer);
        resetTimer = setTimeout(() => {
          label.textContent = col;
        }, 1200);
      };

      container.append(colorStrip);
    });
    return () => {
      clearTimeout(resetTimer);
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <div class="inspector-notice">${t('paletteNotice', '每日色彩搭配灵感，在色块上悬停查看代码，点击直接复制到剪贴板。')}</div>
      <button id="p-next-btn" type="button" class="primary">${t('nextPalette', '换一组调色板')}</button>
    `;
    wrap.querySelector('#p-next-btn').onclick = () => {
      const idx = Number(data.paletteIndex) || 0;
      onChange({ ...data, paletteIndex: (idx + 1) % SAMPLE_PALETTES.length });
    };
    container.append(wrap);
  },
  styles: `
    .Palette {
      display:flex;
      width: 500px;
      height: 200px;
      overflow: hidden;
      border-radius:12px;
      box-shadow:0 4px 12px rgba(0,0,0,.2);
    }
    .Palette .Color {
      flex: 1;
      display: flex;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      position: relative;
      transition:flex .3s ease;
    }
    .Palette .Color:hover {
      flex:1.5;
      z-index: 1;
    }
    .Palette .Color .label {
      position: absolute;
      font-size:.8rem;
      font-weight:600;
      opacity: 0;
      pointer-events: none;
      font-family:monospace;
      text-transform: uppercase;
      transform: rotate(-90deg);
      white-space: nowrap;
      text-shadow:0 1px 2px rgba(0,0,0,.2);
      top:50%; left:50%; margin-top:-.5em; display:block; text-align:center; width:150px;
      margin-left:-75px; transition:opacity .3s ease;
    }
    .Palette .Color:hover .label {
      opacity:.9;
    }
  `,
};
