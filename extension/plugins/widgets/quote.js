

import { escapeHtml } from '../sanitizer.js';

const AI_TECH_QUOTES = [
  { text: 'We can only see a short distance ahead, but we can see plenty there that needs to be done.', author: 'Alan Turing' },
  { text: 'Information is the resolution of uncertainty.', author: 'Claude Shannon' },
  { text: 'If people do not believe that mathematics is simple, it is only because they do not realize how complicated life is.', author: 'John von Neumann' },
  { text: 'The best way to predict the future is to invent it.', author: 'Alan Kay' },
  { text: 'Intelligence is the ability to adapt to change.', author: 'Stephen Hawking' },
  { text: 'Stay hungry, stay foolish.', author: 'Steve Jobs' },
];

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
        <button type="button" id="q-dice" class="quote-random-btn">
          <svg class="icon" aria-hidden="true" style="width:13px;height:13px;"><use href="#i-refresh" /></svg>
          <span>随机换一句科技先驱名言</span>
        </button>
      </div>
    `;
    const update = () => onChange({
      text: container.querySelector('#q-txt').value,
      author: container.querySelector('#q-auth').value,
    });
    container.querySelector('#q-txt').onchange = update;
    container.querySelector('#q-auth').onchange = update;
    container.querySelector('#q-dice').onclick = () => {
      const pick = AI_TECH_QUOTES[Math.floor(Math.random() * AI_TECH_QUOTES.length)];
      container.querySelector('#q-txt').value = pick.text;
      container.querySelector('#q-auth').value = pick.author;
      onChange(pick);
    };
  },
  styles: `
    .Quote { overflow-y:hidden; max-height:33vh; }
    .Quote:hover { overflow-y: auto; }
    .Quote h4 { line-height:1.25 !important; }
    .Quote sub { bottom:0; }
  `,
};
