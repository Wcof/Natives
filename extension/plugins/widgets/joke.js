/** Joke widget. */

import { fetchDedup } from '../plugins-cache.js';

const OFFLINE_JOKES = [
  "Why do programmers prefer dark mode? Because light attracts bugs.",
  "There are 10 types of people: those who understand binary, and those who don't.",
  "Why did the developer go broke? Because they used up all their cache.",
  "A SQL query walks into a bar, walks up to two tables and asks: 'Can I join you?'",
  "How do you comfort a JavaScript bug? You console it.",
];

export const jokeWidget = {
  key: 'widget/joke',
  name: 'Joke',
  defaultData: {},
  render(container) {
    const randomOffline = OFFLINE_JOKES[Math.floor(Math.random() * OFFLINE_JOKES.length)];
    container.className = 'Widget joke-container';
    container.replaceChildren();

    const textEl = document.createElement('div');
    textEl.className = 'question-joke-setup';
    textEl.textContent = `“${randomOffline}”`;
    container.append(textEl);
    let disposed = false;

    fetchDedup(
      'daily_joke',
      async () => {
        const res = await fetch('https://v2.jokeapi.dev/joke/Programming?safe-mode&type=single');
        const json = await res.json();
        return json?.joke ? { joke: json.joke } : null;
      },
      60 * 60 * 1000,
    )
      .then((cached) => {
        if (!disposed && cached?.joke) {
          textEl.textContent = `“${cached.joke}”`;
        }
      })
      .catch(() => {});
    return () => { disposed = true; };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `<div class="inspector-notice">${t('jokeNotice', '定期获取编程幽默与趣味笑话。')}</div>`;
  },
  styles: `
    .joke-container .question-joke-setup:hover { cursor:pointer; }
  `,
};
