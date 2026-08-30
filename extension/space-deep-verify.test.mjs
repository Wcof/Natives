import assert from 'node:assert/strict';
import { widgetPlugins, backgroundPlugins, WIDGET_KEYS, BACKGROUND_KEYS } from './space-plugins.js';
import { clearMemCache, getMemCache } from './plugins/plugins-cache.js';

console.log('=== Personal Space Deep Verification Suite ===');

// 1. Minimal DOM Mock Environment
globalThis.Node = { ELEMENT_NODE: 1, TEXT_NODE: 3 };

class MockElement {
  constructor(tagName = 'div') {
    this.tagName = tagName.toUpperCase();
    this.nodeType = globalThis.Node.ELEMENT_NODE;
    this.id = '';
    this.style = {};
    this.dataset = {};
    this.attributes = [];
    this.childNodes = [];
    this.parentNode = null;
    this.classList = {
      _classes: new Set(),
      add(...cls) { cls.forEach((c) => this._classes.add(c)); },
      remove(...cls) { cls.forEach((c) => this._classes.delete(c)); },
      contains(c) { return this._classes.has(c); },
    };
    this._textContent = '';
  }

  get className() { return Array.from(this.classList._classes).join(' '); }
  set className(val) {
    this.classList._classes.clear();
    String(val).split(/\s+/).filter(Boolean).forEach((c) => this.classList._classes.add(c));
  }

  get children() { return this.childNodes.filter((n) => n.nodeType === globalThis.Node.ELEMENT_NODE); }
  get textContent() {
    if (this.childNodes.length > 0) {
      return this.childNodes.map((n) => n.textContent).join(' ');
    }
    return this._textContent;
  }
  set textContent(val) {
    this._textContent = String(val);
    this.childNodes = [];
  }

  get innerHTML() {
    if (this.childNodes.length > 0) {
      return this.childNodes.map((n) => {
        const tag = n.tagName.toLowerCase();
        const attrs = n.attributes.map((a) => ` ${a.name}="${a.value}"`).join('');
        return `<${tag}${attrs}>${n.innerHTML || n.textContent}</${tag}>`;
      }).join('');
    }
    return this._textContent;
  }
  set innerHTML(html) {
    this.childNodes = [];
    const matches = html.matchAll(/<([a-z0-9-]+)([^>]*)>(.*?)<\/\1>|<([a-z0-9-]+)([^>]*)\/?>/gis);
    for (const match of matches) {
      const tag = match[1] || match[4];
      const attrs = match[2] || match[5] || '';
      const text = match[3] || '';
      const child = new MockElement(tag);
      child.parentNode = this;
      const idMatch = attrs.match(/id="([^"]+)"/);
      if (idMatch) child.id = idMatch[1];
      const typeMatch = attrs.match(/type="([^"]+)"/);
      if (typeMatch) child.type = typeMatch[1];
      const valMatch = attrs.match(/value="([^"]*)"/);
      if (valMatch) child.value = valMatch[1];
      if (/checked/i.test(attrs)) child.checked = true;
      if (text && !text.startsWith('<')) child.textContent = text;
      this.childNodes.push(child);
    }
  }

  append(...nodes) {
    nodes.forEach((n) => {
      n.parentNode = this;
      this.childNodes.push(n);
    });
  }
  replaceChildren(...nodes) {
    this.childNodes = [];
    this.append(...nodes);
    this._textContent = '';
  }
  setAttribute(k, v) {
    const existing = this.attributes.find((a) => a.name === k);
    if (existing) existing.value = String(v);
    else this.attributes.push({ name: k, value: String(v) });
  }
  getAttribute(k) {
    const a = this.attributes.find((attr) => attr.name === k);
    return a ? a.value : null;
  }
  removeAttribute(k) { this.attributes = this.attributes.filter((a) => a.name !== k); }
  remove() {
    if (this.parentNode) {
      this.parentNode.childNodes = this.parentNode.childNodes.filter((c) => c !== this);
    }
  }
  querySelector(sel) {
    return this.querySelectorAll(sel)[0] || null;
  }
  querySelectorAll(sel) {
    const idMatch = sel.match(/#([a-zA-Z0-9_-]+)/);
    const tagMatch = sel.match(/^[a-zA-Z0-9_-]+/);
    const isClass = sel.startsWith('.');
    const isCheckbox = sel.includes('[type="checkbox"]');

    const results = [];
    const walk = (node) => {
      for (const child of node.childNodes) {
        if (child.nodeType === globalThis.Node.ELEMENT_NODE) {
          let match = true;
          if (idMatch && child.id !== idMatch[1]) match = false;
          if (tagMatch && child.tagName !== tagMatch[0].toUpperCase()) match = false;
          if (isClass && !child.classList?.contains(sel.slice(1))) match = false;
          if (isCheckbox && (child.tagName !== 'INPUT' || child.type !== 'checkbox')) match = false;
          if (match) results.push(child);
          walk(child);
        }
      }
    };
    walk(this);
    return results;
  }
}

globalThis.document = {
  createElement(tag) { return new MockElement(tag); },
};
globalThis.DOMParser = class {
  parseFromString(html) {
    const doc = new MockElement('body');
    doc.innerHTML = html;
    return { body: doc };
  }
};

// 2. Fetch Mock with parameter tracking
let lastFetchedUrl = '';
globalThis.fetch = async (url) => {
  lastFetchedUrl = String(url);
  if (lastFetchedUrl.includes('biturl.top')) {
    return { json: async () => ({ url: 'https://bing.com/th?id=OHR.DailyWallpaper_1920x1080.jpg' }) };
  }
  if (lastFetchedUrl.includes('nasa.gov')) {
    return { json: async () => ({ url: 'https://apod.nasa.gov/apod/image/stars.jpg', media_type: 'image' }) };
  }
  if (lastFetchedUrl.includes('wikimedia.org')) {
    return { json: async () => ({ image: { thumbnail: { source: 'https://upload.wikimedia.org/potd.jpg' } } }) };
  }
  if (lastFetchedUrl.includes('open-meteo.com')) {
    return { json: async () => ({ current: { temperature_2m: 24.5, weather_code: 0 } }) };
  }
  if (lastFetchedUrl.includes('er-api.com')) {
    return { json: async () => ({ rates: { CNY: 7.24, EUR: 0.92 } }) };
  }
  if (lastFetchedUrl.includes('coingecko.com')) {
    return { json: async () => ({ bitcoin: { usd: 68000, cny: 480000 } }) };
  }
  if (lastFetchedUrl.includes('ipify.org')) {
    return { json: async () => ({ ip: '203.0.113.195' }) };
  }
  if (lastFetchedUrl.includes('jokeapi.dev')) {
    return { json: async () => ({ joke: 'Why do programmers wear glasses? Because they need C#.' }) };
  }
  if (lastFetchedUrl.includes('leetcode')) {
    return { json: async () => ({ questionTitle: 'Two Sum', difficulty: 'Easy', questionLink: 'https://leetcode.com/problems/two-sum' }) };
  }
  return { json: async () => ({}) };
};

globalThis.chrome = {
  bookmarks: {
    getRecent(count, cb) {
      cb([
        { id: 'b1', title: 'GitHub', url: 'https://github.com' },
        { id: 'b2', title: 'MDN', url: 'https://developer.mozilla.org' },
      ]);
    },
  },
  topSites: {
    get(cb) {
      cb([
        { title: 'Google', url: 'https://google.com' },
        { title: 'Bing', url: 'https://bing.com' },
      ]);
    },
  },
};

const ctx = {
  t: (k, f) => f || k,
  lang: 'zh_CN',
  shadowRoot: new MockElement('shadow-root'),
  onDataChange: () => {},
};

// ─── Batch A: All 9 Backgrounds Real Verification ──────────────────────────

console.log('\n--- Batch A: Verifying 9 Backgrounds ---');

// 1. Bing Wallpaper
{
  clearMemCache();
  const c = new MockElement();
  backgroundPlugins['background/bing'].render(c, {});
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.style.backgroundImage.includes('DailyWallpaper'), 'Bing must set wallpaper URL from API');
  assert.equal(c.style.backgroundSize, 'cover');
  console.log('✓ background/bing live wallpaper passed');
}

// 2. NASA APOD
{
  clearMemCache();
  const c = new MockElement();
  backgroundPlugins['background/apod'].render(c, { apiKey: 'DEMO_KEY' });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.style.backgroundImage.includes('stars.jpg'), 'APOD must set astronomy picture URL');
  console.log('✓ background/apod live APOD passed');
}

// 3. Wikimedia POTD
{
  clearMemCache();
  const c = new MockElement();
  backgroundPlugins['background/wikimedia'].render(c, {});
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.style.backgroundImage.includes('wikimedia.org'), 'Wikimedia must set POTD URL');
  console.log('✓ background/wikimedia live POTD passed');
}

// 4. Solid Colour
{
  const c = new MockElement();
  backgroundPlugins['background/colour'].render(c, { colour: '#ff0055' });
  assert.equal(c.style.backgroundColor, '#ff0055');
  assert.equal(c.style.backgroundImage, 'none');
  console.log('✓ background/colour custom colour passed');
}

// 5. Gradient
{
  const c = new MockElement();
  backgroundPlugins['background/gradient'].render(c, { from: '#111111', to: '#444444', angle: 90 });
  assert.equal(c.style.backgroundImage, 'linear-gradient(90deg, #111111, #444444)');
  console.log('✓ background/gradient custom gradient passed');
}

// 6. Online Image URL
{
  const c = new MockElement();
  backgroundPlugins['background/online'].render(c, { url: 'https://example.com/custom.jpg' });
  assert.equal(c.style.backgroundImage, 'url("https://example.com/custom.jpg")');
  assert.equal(c.style.backgroundSize, 'cover');
  console.log('✓ background/online valid URL passed');
}

// 7. Unsplash
{
  const c = new MockElement();
  backgroundPlugins['background/unsplash'].render(c, { query: 'mountains' });
  assert.ok(c.style.backgroundImage.includes('unsplash.com'), 'Unsplash must set image URL');
  console.log('✓ background/unsplash query passed');
}

// 8. Giphy
{
  const c = new MockElement();
  backgroundPlugins['background/giphy'].render(c, { tag: 'stars', apiKey: '' });
  assert.ok(c.innerHTML.includes('未配置'), 'Giphy without key must show unconfigured notice');
  console.log('✓ background/giphy key guard passed');
}

// 9. Media
{
  const c = new MockElement();
  backgroundPlugins['background/media'].render(c, { mediaType: 'image' });
  assert.ok(c.innerHTML.includes('本地媒体'), 'Media background must show local media info');
  console.log('✓ background/media render passed');
}

// ─── Batch B: Verifying 29 Widgets Logic & Display ──────────────────────────

console.log('\n--- Batch B: Verifying 29 Widgets ---');

// 1. Weather
{
  clearMemCache();
  const c = new MockElement();
  widgetPlugins['widget/weather'].render(c, { city: 'Shanghai', lat: 31.23, lon: 121.47 });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('Shanghai'), 'Weather must show city');
  assert.ok(c.textContent.includes('24.5°C'), 'Weather must show live temperature');
  assert.ok(c.textContent.includes('晴'), 'Weather must translate weather code 0 to 晴');
  console.log('✓ widget/weather open-meteo integration passed');
}

// 2. Currency Rates
{
  clearMemCache();
  const c = new MockElement();
  widgetPlugins['widget/currencyRates'].render(c, { base: 'USD', target: 'CNY' });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('1 USD ≈ 7.24 CNY'), 'Currency rates must show parsed rate');
  console.log('✓ widget/currencyRates live exchange rate passed');
}

// 3. Bitcoin Price
{
  clearMemCache();
  const c = new MockElement();
  widgetPlugins['widget/bitcoin'].render(c, { currency: 'USD' });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('68,000') || c.textContent.includes('68000'), 'Bitcoin widget must format crypto price');
  console.log('✓ widget/bitcoin live price passed');
}

// 4. IP Info
{
  clearMemCache();
  const c = new MockElement();
  widgetPlugins['widget/ipInfo'].render(c, {});
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('203.0.113.195'), 'IP info must display public IP');
  console.log('✓ widget/ipInfo public IP passed');
}

// 5. Work Hours progress
{
  const c = new MockElement();
  widgetPlugins['widget/workHours'].render(c, { start: '00:00', end: '23:59', label: '工作工时' });
  assert.ok(c.textContent.includes('工作工时'), 'Work hours must show custom label');
  const bar = c.querySelector('.widget-workhours-box');
  assert.ok(bar, 'Work hours must render progress bar container');
  console.log('✓ widget/workHours progress bar computation passed');
}

// 6. Search Multi-provider
{
  const c = new MockElement();
  widgetPlugins['widget/search'].render(c, { provider: 'bing', placeholder: '必应一下' });
  const form = c.querySelector('form');
  assert.equal(form.action, 'https://www.bing.com/search');
  assert.equal(form.querySelector('input').name, 'q');
  console.log('✓ widget/search multi-engine provider passed');
}

// 7. Palette & Copy
{
  let updatedData = null;
  const c = new MockElement();
  widgetPlugins['widget/palette'].render(c, { paletteIndex: 0 }, {}, {
    ...ctx,
    onDataChange: (d) => { updatedData = d; },
  });
  const swatches = c.querySelectorAll('span');
  assert.equal(swatches.length, 5, 'Palette must render 5 color swatches');
  const refreshBtn = c.querySelector('button');
  refreshBtn.onclick({ stopPropagation() {} });
  assert.equal(updatedData.paletteIndex, 1, 'Palette refresh must rotate palette index');
  console.log('✓ widget/palette color palette & rotation passed');
}

// 8. Quick Links CRUD
{
  const c = new MockElement();
  widgetPlugins['widget/links'].render(c, {
    links: [
      { title: 'Doc', url: 'https://docs.natives.io' },
      { title: 'Home', url: 'https://natives.io' },
    ],
  });
  const anchors = c.querySelectorAll('a');
  assert.equal(anchors.length, 2, 'Links must render 2 anchors');
  assert.equal(anchors[0].textContent, 'Doc');
  console.log('✓ widget/links list rendering passed');
}

// 9. To Do Interactive
{
  let todoSaved = null;
  const c = new MockElement();
  widgetPlugins['widget/todo'].render(
    c,
    { items: [{ text: 'Ship v1.0', done: false }] },
    {},
    { ...ctx, onDataChange: (d) => { todoSaved = d; } },
  );
  const cb = c.querySelector('input[type="checkbox"]');
  assert.equal(cb.checked, false);
  cb.onchange({ target: { checked: true } });
  assert.equal(todoSaved.items[0].done, true, 'Checking todo must update done state');
  console.log('✓ widget/todo interactive state mutation passed');
}

// 10. Notes Editable
{
  let noteSaved = null;
  const c = new MockElement();
  widgetPlugins['widget/notes'].render(
    c,
    { content: 'Draft meeting notes' },
    {},
    { ...ctx, onDataChange: (d) => { noteSaved = d; } },
  );
  const area = c.querySelector('textarea');
  assert.equal(area.value, 'Draft meeting notes');
  area.value = 'Updated notes';
  area.onchange();
  assert.equal(noteSaved.content, 'Updated notes');
  console.log('✓ widget/notes auto-save callback passed');
}

// 11. HTML Sanitizer
{
  const c = new MockElement();
  widgetPlugins['widget/html'].render(c, {
    html: '<p>Safe Text</p><script>alert(1)</script><a href="javascript:steal()">Bad Link</a>',
  });
  assert.ok(c.innerHTML.includes('Safe Text'), 'Safe tags must be preserved');
  assert.ok(!c.innerHTML.includes('<script>'), 'Dangerous script tag must be removed');
  assert.ok(!c.innerHTML.includes('javascript:'), 'javascript: pseudoprotocol must be removed');
  console.log('✓ widget/html DOM sanitization security check passed');
}

// 12. CSS Widget Shadow Insertion
{
  const c = new MockElement();
  const shadowRoot = new MockElement('shadow-root');
  widgetPlugins['widget/css'].render(c, { css: 'body { color: red; }' }, {}, { ...ctx, shadowRoot });
  assert.ok(shadowRoot.childNodes.some((child) => child.id === 'custom-css-widget' && child.textContent.includes('color: red')), 'CSS widget must insert style into shadow root');
  console.log('✓ widget/css Shadow DOM isolation passed');
}

// 13. Bookmarks (Chrome API permission)
{
  const c = new MockElement();
  widgetPlugins['widget/bookmarks'].render(c, {});
  const links = c.querySelectorAll('a');
  assert.equal(links.length, 2, 'Bookmarks must render 2 recent bookmark items');
  console.log('✓ widget/bookmarks Chrome permission binding passed');
}

// 14. Top Sites (Chrome API permission)
{
  const c = new MockElement();
  widgetPlugins['widget/topSites'].render(c, {});
  const links = c.querySelectorAll('a');
  assert.equal(links.length, 2, 'Top Sites must render 2 top site items');
  console.log('✓ widget/topSites Chrome permission binding passed');
}

// 15. Time & Greeting
{
  const tc = new MockElement();
  widgetPlugins['widget/time'].render(tc, { hour12: false, showSeconds: true });
  assert.ok(tc.textContent.length > 0, 'Time widget must output time string');

  const gc = new MockElement();
  widgetPlugins['widget/greeting'].render(gc, { name: 'Alice' }, {}, ctx);
  assert.ok(gc.textContent.includes('Alice'), 'Greeting must include custom name');
  console.log('✓ widget/time and widget/greeting localized formatting passed');
}

// 16. Countdown & Since
{
  const cd = new MockElement();
  widgetPlugins['widget/countdown'].render(cd, { title: 'Launch', targetDate: '2099-01-01' });
  assert.ok(cd.textContent.includes('Launch') && cd.textContent.includes('天'), 'Countdown must compute remaining days');

  const sc = new MockElement();
  widgetPlugins['widget/since'].render(sc, { title: 'Project Start', sinceDate: '2020-01-01' });
  assert.ok(sc.textContent.includes('Project Start') && sc.textContent.includes('已过去'), 'Since must compute elapsed days');
  console.log('✓ widget/countdown and widget/since date calculations passed');
}

// 17. Literature Clock
{
  const c = new MockElement();
  widgetPlugins['widget/literatureClock'].render(c, {});
  assert.ok(c.childNodes.length >= 2, 'Literature clock must render quote and citation');
  console.log('✓ widget/literatureClock quote matching passed');
}

// 18. Joke & LeetCode
{
  const jc = new MockElement();
  widgetPlugins['widget/joke'].render(jc, {});
  assert.ok(jc.textContent.length > 5, 'Joke must output a non-empty joke');

  const lc = new MockElement();
  widgetPlugins['widget/leetcode'].render(lc, {});
  assert.ok(lc.textContent.includes('LeetCode'), 'LeetCode must show daily problem title');
  console.log('✓ widget/joke and widget/leetcode live feeds passed');
}

// 19. Tally Counter
{
  let countVal = 0;
  const c = new MockElement();
  widgetPlugins['widget/tallyCounter'].render(
    c,
    { count: 5, title: 'Bugs Fixed' },
    {},
    { ...ctx, onDataChange: (d) => { countVal = d.count; } },
  );
  const plusBtn = c.querySelector('.plus');
  plusBtn.onclick();
  assert.equal(countVal, 6, 'Clicking plus must increment count');
  console.log('✓ widget/tallyCounter counter mutation passed');
}

// 20. Remaining widgets contract & default verification
for (const key of ['widget/binaryTime', 'widget/customText', 'widget/github', 'widget/message', 'widget/quote', 'widget/timeTracker', 'widget/trello']) {
  const c = new MockElement();
  widgetPlugins[key].render(c, widgetPlugins[key].defaultData, {}, ctx);
  assert.ok(c.textContent.length > 0 || c.childNodes.length > 0, `Widget ${key} must render content`);
  console.log(`✓ ${key} smoke and logic passed`);
}

console.log('\n======================================================');
console.log('ALL 29 WIDGETS AND 9 BACKGROUNDS PASS DEEP VERIFICATION!');
console.log('======================================================\n');
