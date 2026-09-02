/** Quote widget using the Tabliss QuoteContent structure. */

import { escapeHtml } from '../sanitizer.js';

export const quoteWidget = {
  key: 'widget/quote',
  name: 'Quote',
  defaultData: { text: 'Stay hungry, stay foolish.', author: 'Steve Jobs' },
  render(container, data) {
    container.className = 'Widget Quote';
    container.replaceChildren();

    const quote = document.createElement('h4');
    quote.textContent = `“${data.text || ''}”`;

    const cite = document.createElement('sub');
    cite.innerHTML = `<br>— ${escapeHtml(data.author || 'Anonymous')}`;

    container.append(quote, cite);
  },
  renderSettings(container, data, onChange, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('quote', '名言')}</span><textarea rows="2" id="q-txt">${escapeHtml(data.text || '')}</textarea></label>
        <label class="inspector-field"><span>${t('author', '作者')}</span><input type="text" id="q-auth" value="${escapeHtml(data.author || '')}" /></label>
      </div>
    `;
    const update = () => onChange({
      text: container.querySelector('#q-txt').value,
      author: container.querySelector('#q-auth').value,
    });
    container.querySelector('#q-txt').onchange = update;
    container.querySelector('#q-auth').onchange = update;
  },
  styles: `
    .Quote { overflow-y:hidden; max-height:33vh; }
    .Quote:hover { overflow-y: auto; }
    .Quote h4 { line-height:1.25 !important; }
    .Quote sub { bottom:0; }
  `,
};
