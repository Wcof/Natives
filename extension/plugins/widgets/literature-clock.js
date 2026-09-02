/** Literature clock widget with Tabliss quote/citation hierarchy. */

const QUOTES_SAMPLE = [
  { h: 0, m: 0, quote: "It was midnight. The clock struck twelve, and the world belonged to ghosts.", book: "Cinderella", author: "Charles Perrault" },
  { h: 7, m: 0, quote: "Seven o'clock in the morning. The sun was up, the birds were singing.", book: "The Great Gatsby", author: "F. Scott Fitzgerald" },
  { h: 9, m: 0, quote: "At nine o'clock in the morning, the light was clear and sharp as crystal.", book: "Mrs Dalloway", author: "Virginia Woolf" },
  { h: 12, m: 0, quote: "Big Ben struck twelve. Its leaden circles dissolved in the air.", book: "Mrs Dalloway", author: "Virginia Woolf" },
  { h: 13, m: 0, quote: "It was a bright cold day in April, and the clocks were striking thirteen.", book: "1984", author: "George Orwell" },
  { h: 18, m: 0, quote: "Six o'clock in the evening, and the streetlamps began to flicker into life.", book: "The Picture of Dorian Gray", author: "Oscar Wilde" },
  { h: 22, m: 30, quote: "It was half-past ten when they finally reached the gates of the castle.", book: "Dracula", author: "Bram Stoker" },
];

export const literatureClockWidget = {
  key: 'widget/literatureClock',
  name: 'Literature Clock',
  defaultData: {},
  render(container) {
    const now = new Date();
    const h = now.getHours();
    const m = now.getMinutes();

    const matched = QUOTES_SAMPLE.find((q) => q.h === h && Math.abs(q.m - m) <= 10) || QUOTES_SAMPLE.find((q) => q.h === h) || QUOTES_SAMPLE[4];

    container.className = 'Widget LiteratureClock';
    container.replaceChildren();

    const quoteEl = document.createElement('blockquote');
    quoteEl.innerHTML = `<strong>${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}</strong> <span>“${matched.quote}”</span>`;

    const citeEl = document.createElement('cite');
    citeEl.textContent = `— ${matched.author}, ${matched.book}`;

    container.append(quoteEl, citeEl);
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `<div class="inspector-notice">${t('literatureClockNotice', '根据当前时刻自动匹配文学作品中的时刻段落。')}</div>`;
  },
  styles: `
    .LiteratureClock blockquote { text-align:justify; line-height:1.6em; max-width:50vw; }
    .LiteratureClock span { opacity:.9; }
    .LiteratureClock strong { opacity:1; font-size:1.5em; }
    .LiteratureClock cite { display:block; text-align:right; font-style:normal; font-size:.7em; margin-right:2rem; }
  `,
};
