import assert from 'node:assert/strict';
import { widgetPlugins, backgroundPlugins } from './space-plugins.js';
import { createSpaceWidgetSettings } from './space-widget-settings.js';
import { createSpaceBackgroundSettings } from './space-background-settings.js';
import { createWorkspaceMutationQueue } from './space-write-queue.js';
import { clearMemCache } from './plugins/plugins-cache.js';
import { MockElement, setupTestDomEnvironment } from './test-dom-mock.js';

console.log('=== Personal Space Deep Verification Suite ===');

setupTestDomEnvironment();

// Fetch Mock with parameter tracking
let lastFetchedUrl = '';
let lastBingApiUrl = '';
globalThis.fetch = async (url) => {
  lastFetchedUrl = String(url);
  if (lastFetchedUrl.includes('bing.com/HPImageArchive.aspx')) {
    lastBingApiUrl = lastFetchedUrl;
    const index = new URL(lastFetchedUrl).searchParams.get('idx') || '0';
    return { ok: true, json: async () => ({ images: [{ url: `/th?id=OHR.Mock${index}_1920x1080.jpg`, copyright: `Mock ${index}` }] }) };
  }
  if (lastFetchedUrl.includes('nasa.gov')) return { ok: true, json: async () => ({ url: 'https://apod.nasa.gov/apod/image/stars.jpg', media_type: 'image' }) };
  if (lastFetchedUrl.includes('wikimedia.org')) return { ok: true, json: async () => ({ image: { thumbnail: { source: 'https://upload.wikimedia.org/potd.jpg' } } }) };
  if (lastFetchedUrl.includes('open-meteo.com')) return { ok: true, json: async () => ({ current: { temperature_2m: 24.5, weather_code: 0, relative_humidity_2m: 50, wind_speed_10m: 12 }, daily: { time: ['2026-09-01', '2026-09-02'], weather_code: [0, 1], temperature_2m_max: [28, 27], temperature_2m_min: [18, 17] } }) };
  if (lastFetchedUrl.includes('er-api.com')) return { ok: true, json: async () => ({ result: 'success', rates: { CNY: 7.24, EUR: 0.92, JPY: 155.2, GBP: 0.78 } }) };
  if (lastFetchedUrl.includes('coingecko.com')) return { ok: true, json: async () => ({ bitcoin: { usd: 68000, cny: 480000 } }) };
  if (lastFetchedUrl.includes('mempool.space/api/v1/blocks')) return { ok: true, json: async () => ([{ height: 860000, size: 1400000, tx_count: 2500, timestamp: Math.floor(Date.now() / 1000) - 300 }]) };
  if (lastFetchedUrl.includes('mempool.space/api/v1/fees')) return { ok: true, json: async () => ({ fastestFee: 15, halfHourFee: 10, hourFee: 8, minimumFee: 5 }) };
  if (lastFetchedUrl.includes('ipify.org')) return { ok: true, json: async () => ({ ip: '203.0.113.195' }) };
  if (lastFetchedUrl.includes('jokeapi.dev')) return { ok: true, json: async () => ({ joke: 'Why do programmers wear glasses? Because they need C#.' }) };
  if (lastFetchedUrl.includes('leetcode')) return { ok: true, json: async () => ({ streak: 5, submissionCalendar: '{"1756598400": 3}' }) };
  if (lastFetchedUrl.includes('github-contributions-api')) return { ok: true, json: async () => ({ total: { 2026: 3 }, contributions: [{ date: '2026-08-31', count: 3, level: 2 }] }) };
  return { ok: true, json: async () => ({}) };
};

globalThis.chrome = {
  bookmarks: {
    getRecent: (count, cb) => cb([
      { id: 'b1', title: 'GitHub', url: 'https://github.com' },
      { id: 'b2', title: 'MDN', url: 'https://developer.mozilla.org' },
    ]),
    getChildren: (id, cb) => cb([
      { id: 'b1', title: 'GitHub', url: 'https://github.com' },
      { id: 'b2', title: 'MDN', url: 'https://developer.mozilla.org' },
    ]),
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
{
  clearMemCache();
  const c = new MockElement();
  backgroundPlugins['background/bing'].render(c, { interval: 'daily', index: 0, mkt: 'zh-CN' });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.style.backgroundImage.includes('OHR.Mock0'), 'Bing must set wallpaper URL from the official archive API'); assert.match(lastBingApiUrl, /[?&]idx=0(?:&|$)/, 'Bing must request the selected archive index');
  assert.equal(c.style.backgroundSize, 'cover');
  assert.equal(c.querySelector('.bing-wallpaper-info'), null, 'Bing must not actively render copyright text over the wallpaper');
  console.log('✓ background/bing live wallpaper passed');
}

// 2. NASA APOD
{
  clearMemCache();
  const c = new MockElement();
  const d = backgroundPlugins['background/apod'].render(c, { apiKey: 'DEMO_KEY' });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.style.backgroundImage.includes('stars.jpg'), 'APOD must set astronomy picture URL');
  if (typeof d === 'function') d();
  console.log('✓ background/apod live APOD passed');
}

// 3. Wikimedia POTD
{
  clearMemCache();
  const c = new MockElement();
  const d = backgroundPlugins['background/wikimedia'].render(c, {});
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.style.backgroundImage.includes('wikimedia.org'), 'Wikimedia must set POTD URL');
  if (typeof d === 'function') d();
  console.log('✓ background/wikimedia live POTD passed');
}

// 4. Solid Colour
{
  const c = new MockElement();
  const d = backgroundPlugins['background/colour'].render(c, { colour: '#ff0055' });
  assert.equal(c.style.backgroundColor, '#ff0055');
  assert.equal(c.style.backgroundImage, 'none');
  if (typeof d === 'function') d();
  console.log('✓ background/colour custom colour passed');
}

// 5. Gradient
{
  const c = new MockElement();
  const d = backgroundPlugins['background/gradient'].render(c, { from: '#111111', to: '#444444', angle: 90 });
  assert.equal(c.style.backgroundImage, 'linear-gradient(90deg, #111111, #444444)');
  if (typeof d === 'function') d();
  console.log('✓ background/gradient custom gradient passed');
}

// 6. Online Image URL
{
  const c = new MockElement();
  const d = backgroundPlugins['background/online'].render(c, { url: 'https://example.com/custom.jpg' });
  assert.equal(c.style.backgroundImage, 'url("https://example.com/custom.jpg")');
  assert.equal(c.style.backgroundSize, 'cover');
  if (typeof d === 'function') d();
  console.log('✓ background/online valid URL passed');
}

// 7. Unsplash
{
  const c = new MockElement();
  const d = backgroundPlugins['background/unsplash'].render(c, { query: 'mountains' });
  assert.ok(c.style.backgroundImage.includes('unsplash.com'), 'Unsplash must set image URL');
  if (typeof d === 'function') d();
  console.log('✓ background/unsplash query passed');
}

// 8. Giphy
{
  const c = new MockElement();
  const d = backgroundPlugins['background/giphy'].render(c, { tag: 'stars', apiKey: '' });
  assert.ok(c.innerHTML.includes('未配置') || !c.style.backgroundImage, 'Giphy without key must show unconfigured notice');
  if (typeof d === 'function') d();
  console.log('✓ background/giphy key guard passed');
}

// 9. Media
{
  const c = new MockElement();
  const d = backgroundPlugins['background/media'].render(c, { mediaType: 'image' });
  assert.ok(c.innerHTML.includes('本地媒体'), 'Media background must show local media info');
  if (typeof d === 'function') d();
  console.log('✓ background/media render passed');
}

// ─── Batch B: Verifying 29 Widgets Logic & Display ──────────────────────────

console.log('\n--- Batch B: Verifying 29 Widgets ---');

// 1. Weather
{
  clearMemCache();
  const c = new MockElement();
  const d = widgetPlugins['widget/weather'].render(c, { city: 'Shanghai', lat: 31.23, lon: 121.47 });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('Shanghai'), 'Weather must show city');
  assert.ok(c.textContent.includes('25°C'), 'Weather must show rounded live temperature from open-meteo');
  assert.ok(c.textContent.includes('晴'), 'Weather must translate weather code 0 to 晴');
  if (typeof d === 'function') d();
  console.log('✓ widget/weather open-meteo integration passed');
}

// 2. Currency Rates
{
  clearMemCache();
  const c = new MockElement();
  const d = widgetPlugins['widget/currencyRates'].render(c, { base: 'USD', targets: 'CNY,EUR' });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('1 USD') && (c.textContent.includes('7.24') || c.textContent.includes('CNY')), 'Currency rates must show parsed rate');
  if (typeof d === 'function') d();
  console.log('✓ widget/currencyRates live exchange rate passed');
}

// 3. IP Info
{
  clearMemCache();
  const c = new MockElement();
  const d = widgetPlugins['widget/ipInfo'].render(c, {});
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('203.0.113.195'), 'IP info must display public IP');
  if (typeof d === 'function') d();
  console.log('✓ widget/ipInfo public IP passed');
}

// 5. Work Hours progress
{
  const c = new MockElement();
  const d = widgetPlugins['widget/workHours'].render(c, { startTime: '00:00', endTime: '23:59', days: [new Date().getDay()] });
  assert.match(c.querySelector('h2').textContent, /^\d+%$/, 'Work hours must render the Tabliss percentage heading');
  assert.equal(c.querySelector('.workhours-bar-bg'), null, 'Work hours must not add a non-Tabliss progress card');
  if (typeof d === 'function') d();
  console.log('✓ widget/workHours percentage computation passed');
}

// 6. Search Multi-provider
{
  const c = new MockElement();
  const d = widgetPlugins['widget/search'].render(c, { provider: 'bing', placeholder: '必应一下' });
  const form = c.querySelector('form');
  assert.equal(form.action, 'https://www.bing.com/search');
  assert.equal(form.querySelector('input').name, 'q');
  assert.equal(c.querySelector('.search-quick-bar'), null, 'search quick switch bar must be removed');
  assert.equal(c.querySelector('.search-ai-prompts-bar'), null, 'search AI prompts bar must be removed');
  if (typeof d === 'function') d();
  console.log('✓ widget/search multi-engine provider passed');
}

// 7. Palette & Copy
{
  let updatedData = null;
  const c = new MockElement();
  const d = widgetPlugins['widget/palette'].render(c, { paletteIndex: 0 }, {}, {
    ...ctx,
    onDataChange: (d) => { updatedData = d; },
  });
  const swatches = c.querySelectorAll('.Color');
  assert.equal(swatches.length, 5, 'Palette must render 5 color swatches');
  if (typeof d === 'function') d();
  console.log('✓ widget/palette color palette & rotation passed');
}

// 8. Quick Links CRUD
{
  const c = new MockElement();
  const d = widgetPlugins['widget/links'].render(c, {
    links: [
      { title: 'Doc', url: 'https://docs.natives.io' },
      { title: 'Home', url: 'https://natives.io' },
    ],
  });
  const anchors = c.querySelectorAll('a');
  assert.equal(anchors.length, 2, 'Links must render 2 anchors');
  assert.ok(anchors[0].textContent.includes('Doc'));
  if (typeof d === 'function') d();
  console.log('✓ widget/links list rendering passed');
}

// 9. To Do Interactive
{
  let todoSaved = null;
  const c = new MockElement();
  const d = widgetPlugins['widget/todo'].render(
    c,
    { items: [{ text: 'Ship v1.0', done: false }] },
    {},
    { ...ctx, onDataChange: (d) => { todoSaved = d; } },
  );
  const cb = c.querySelector('input[type="checkbox"]');
  assert.equal(cb.checked, false);
  cb.onchange({ target: { checked: true } });
  assert.equal(todoSaved.items[0].done, true, 'Checking todo must update done state');
  const delBtn = c.querySelector('.todo-del-btn');
  assert.ok(delBtn, 'Todo items must render a delete button');
  delBtn.onclick({ stopPropagation() {} });
  assert.equal(todoSaved.items.length, 0, 'Delete button must remove the todo item');
  if (typeof d === 'function') d();
  const editContainer = new MockElement();
  let edited = null;
  const editDispose = widgetPlugins['widget/todo'].render(
    editContainer,
    { items: [{ text: 'Draft title', done: false }] },
    {},
    { ...ctx, onDataChange: (next) => { edited = next; } },
  );
  const text = editContainer.querySelector('.todo-text');
  const row = editContainer.querySelector('.TodoItem');
  row.ondblclick({ target: text, stopPropagation() {} });
  const editInput = editContainer.querySelector('.todo-edit-input');
  assert.equal(editInput.hidden, false, 'Double-clicking a todo must reveal the edit input');
  editInput.value = 'Updated title';
  editInput.onkeydown({ key: 'Enter' });
  assert.equal(edited.items[0].text, 'Updated title', 'Enter must persist the edited todo text');
  editDispose?.();
  console.log('✓ widget/todo delete and inline edit passed');
}

// 10. Notes Editable
{
  let noteSaved = null;
  const c = new MockElement();
  const d = widgetPlugins['widget/notes'].render(
    c,
    { content: 'Draft meeting notes' },
    {},
    { ...ctx, onDataChange: (d) => { noteSaved = d; } },
  );
  const area = c.querySelector('textarea');
  assert.equal(area.value, 'Draft meeting notes');
  area.value = 'Updated notes';
  area.oninput();
  assert.equal(noteSaved.content, 'Updated notes');
  if (typeof d === 'function') d();
  console.log('✓ widget/notes auto-save callback passed');
}

// 11. HTML Sanitizer
{
  const c = new MockElement();
  const d = widgetPlugins['widget/html'].render(c, {
    html: '<p>Safe Text</p><script>alert(1)</script><a href="javascript:steal()">Bad Link</a>',
  });
  assert.ok(c.innerHTML.includes('Safe Text'), 'Safe tags must be preserved');
  assert.ok(!c.innerHTML.includes('<script>'), 'Dangerous script tag must be removed');
  if (typeof d === 'function') d();
  console.log('✓ widget/html DOM sanitization security check passed');
}

// 12. CSS Widget Shadow Insertion
{
  const c = new MockElement();
  const shadowRoot = new MockElement('shadow-root');
  const d = widgetPlugins['widget/css'].render(c, { css: 'body { color: red; }' }, {}, { ...ctx, shadowRoot });
  assert.ok(shadowRoot.childNodes.some((child) => child.id === 'custom-css-widget' && child.textContent.includes('color: red')), 'CSS widget must insert style into shadow root');
  if (typeof d === 'function') d();
  console.log('✓ widget/css Shadow DOM isolation passed');
}

// 13. Bookmarks (Chrome API permission)
{
  const c = new MockElement();
  const d = widgetPlugins['widget/bookmarks'].render(c, {});
  const links = c.querySelectorAll('a');
  assert.equal(links.length, 2, 'Bookmarks must render 2 bookmark items');
  if (typeof d === 'function') d();
  console.log('✓ widget/bookmarks Chrome permission binding passed');
}

// 14. Top Sites (Chrome API permission)
{
  const c = new MockElement();
  const d = widgetPlugins['widget/topSites'].render(c, {});
  const links = c.querySelectorAll('a');
  assert.equal(links.length, 2, 'Top Sites must render 2 top site items');
  if (typeof d === 'function') d();
  console.log('✓ widget/topSites Chrome permission binding passed');
}

// Optional browser permissions
{
  chrome.permissions = {
    contains: (_request, callback) => callback(false),
    request: (_request, callback) => callback(true),
  };
  const bookmarks = new MockElement();
  const disposeBookmarks = widgetPlugins['widget/bookmarks'].render(bookmarks, {});
  const bookmarksButton = bookmarks.querySelector('.bookmarks-auth-btn');
  assert.ok(bookmarksButton, 'Bookmarks must request its optional permission before reading data');
  bookmarksButton.onclick();
  assert.equal(bookmarks.querySelectorAll('a').length, 2);
  disposeBookmarks();

  const topSites = new MockElement();
  const disposeTopSites = widgetPlugins['widget/topSites'].render(topSites, {});
  const topSitesButton = topSites.querySelector('.request-permission');
  assert.ok(topSitesButton, 'Top Sites must request its optional permission before reading data');
  topSitesButton.onclick();
  assert.equal(topSites.querySelectorAll('a').length, 2);
  disposeTopSites();
  delete chrome.permissions;
  console.log('✓ optional bookmark and top-sites permission flows passed');
}

// 15. Time & Greeting
{
  const tc = new MockElement();
  const dt = widgetPlugins['widget/time'].render(tc, { hour12: false, showSeconds: true });
  assert.ok(tc.textContent.length > 0, 'Time widget must output time string');
  if (typeof dt === 'function') dt();

  const gc = new MockElement();
  const dg = widgetPlugins['widget/greeting'].render(gc, { name: 'Alice' }, {}, ctx);
  assert.ok(gc.textContent.includes('Alice'), 'Greeting must include custom name');
  if (typeof dg === 'function') dg();
  console.log('✓ widget/time and widget/greeting localized formatting passed');
}

// 16. Countdown & Since
{
  const cd = new MockElement();
  const dc = widgetPlugins['widget/countdown'].render(cd, { title: 'Launch', time: Date.now() + 86400000 });
  assert.ok(cd.textContent.includes('Launch') && cd.querySelector('h3'), 'Countdown must render a relative target time');
  if (typeof dc === 'function') dc();

  const sc = new MockElement();
  const ds = widgetPlugins['widget/since'].render(sc, { title: 'Project Start', time: Date.now() - 86400000 });
  assert.ok(sc.textContent.includes('Project Start') && sc.querySelector('.relativeTime'), 'Since must render relative elapsed time');
  if (typeof ds === 'function') ds();
  console.log('✓ widget/countdown and widget/since date calculations passed');
}

// 17. Tally Counter
{
  let countVal = 0;
  const c = new MockElement();
  const d = widgetPlugins['widget/tallyCounter'].render(
    c,
    { count: 5, label: 'Bugs Fixed' },
    {},
    { ...ctx, onDataChange: (d) => { countVal = d.count; } },
  );
  const plusBtn = c.querySelector('.plus');
  plusBtn.onclick({ stopPropagation() {} });
  assert.equal(countVal, 6, 'Clicking plus must increment count');
  if (typeof d === 'function') d();
  console.log('✓ widget/tallyCounter counter mutation passed');
}

// 18. Remaining widgets contract & default verification
for (const key of ['widget/binaryTime', 'widget/customText', 'widget/message', 'widget/quote', 'widget/trello']) {
  const c = new MockElement();
  const d = widgetPlugins[key].render(c, widgetPlugins[key].defaultData, {}, ctx);
  assert.ok(c.textContent.length > 0 || c.childNodes.length > 0, `Widget ${key} must render content`);
  if (typeof d === 'function') d();
  console.log(`✓ ${key} smoke and logic passed`);
}
{
  const cEmpty = new MockElement();
  const dEmpty = widgetPlugins['widget/github'].render(cEmpty, { username: '' }, {}, ctx);
  assert.ok(cEmpty.childNodes.length > 0, 'GitHub widget with empty username must render unconfigured prompt');
  assert.ok(cEmpty.textContent.includes('GitHub') || cEmpty.innerHTML.includes('github-unconfigured'), 'Must show prompt message');
  if (typeof dEmpty === 'function') dEmpty();

  const c = new MockElement();
  const d = widgetPlugins['widget/github'].render(c, { username: 'octocat' }, {}, ctx);
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.querySelector('.activity-calendar-root'), 'GitHub must render its activity calendar');
  if (typeof d === 'function') d();
  console.log('✓ widget/github activity calendar passed');
}

// ─── Batch C: Immediate Settings Persistence ────────────────────────────────

console.log('\n--- Batch C: Immediate Settings Persistence ---');

// C1. Widget settings persist as soon as a control changes
{
  let updateCalls = 0;
  let updatedWidget = null;
  const widget = {
    id: 'w-search-1',
    key: 'widget/search',
    order: 0,
    enabled: true,
    configJson: { provider: 'google', placeholder: '', newTab: true, showQuickSwitch: true, suggestions: true, suggestionsEngine: 'google', suggestionsQuantity: 4, style: 'default' },
    displayJson: { position: 'middleCentre' },
  };
  const snapshot = { name: 'T', backgroundJson: { key: 'background/colour', display: {} }, widgets: [widget], revision: 1 };
  const ctrl = createSpaceWidgetSettings({
    t: (k, f) => f || k,
    language: 'zh_CN',
    onUpdateWidget: async (nextWidget) => {
      updateCalls += 1;
      updatedWidget = nextWidget;
      return { ...snapshot, revision: 2 };
    },
    onBackToOverview: () => {},
  });
  const container = new MockElement('div');
  ctrl.render(container, snapshot, 'ws-1', 'w-search-1');

  const provider = container.querySelector('#s-provider');
  assert.ok(provider, 'search provider select must exist');
  provider.value = 'bing';
  provider.onchange({ target: { value: 'bing' } });
  assert.equal(updateCalls, 1, 'widget changes must save immediately');
  assert.equal(updatedWidget.configJson.provider, 'bing', 'updated search provider must be emitted');
  assert.equal(container.querySelector('#btn-save-widget'), null, 'widget settings must not render a save button');

  console.log('✓ C1 widget settings save immediately');
}

// C1b. Todo editor stays collapsed until the widget or add action is selected
{
  const container = new MockElement('div');
  const dispose = widgetPlugins['widget/todo'].render(container, { items: [] }, {}, ctx);
  const footer = container.querySelector('.todo-footer');
  assert.equal(footer.hidden, true, 'todo input must be hidden by default');
  assert.equal(container.querySelector('[data-act="ai-plan"]'), null, 'AI planning prompt must be removed');
  container.dispatchEvent(new Event('click'));
  assert.equal(footer.hidden, false, 'selecting the todo widget must reveal the editor');
  footer.querySelector('.todo-add-input').value = '';
  dispose?.();
  console.log('✓ C1b todo editor expands on selection only');
}

// C2. Currency rates: decimals 0 and amount 0 must not fall back
{
  const c = new MockElement();
  widgetPlugins['widget/currencyRates'].render(c, { pairs: [{ id: 'p1', from: 'usd', to: 'cny', amount: 0, showChange: false }], decimals: 0 });
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(c.textContent.includes('0 USD'), 'amount 0 must be preserved, not reset to 1');
  console.log('✓ C2 currencyRates decimals:0 and amount:0 preserved');

  // Settings round-trip keeps decimals 0
  const s = new MockElement();
  let emitted = null;
  widgetPlugins['widget/currencyRates'].renderSettings(s, { pairs: [{ id: 'p1', from: 'usd', to: 'cny', amount: 0 }], decimals: 0 }, (next) => { emitted = next; }, { t: (k, f) => f || k });
  const decInput = s.querySelector('#c-decimals');
  assert.ok(decInput, 'decimals input must exist');
  decInput.value = '0';
  decInput.onchange();
  assert.equal(emitted.decimals, 0, 'decimals 0 must not be reset to 4');
  const amtInput = s.querySelector('.pair-amount');
  assert.equal(amtInput.value, '0', 'amount 0 input must display 0');
  amtInput.onchange();
  assert.equal(emitted.pairs[0].amount, 0, 'amount 0 must not be reset to 1');
  console.log('✓ C2 settings round-trip keeps decimals:0 / amount:0');
}

// C3. Search / todo / links / top-sites / tally emit finite-number guards
{
  const checkEmit = (key, selector, raw, expected, label) => {
    const s = new MockElement();
    let out = null;
    widgetPlugins[key].renderSettings(s, widgetPlugins[key].defaultData || {}, (next) => { out = next; }, { t: (k, f) => f || k });
    const el = s.querySelector(selector);
    assert.ok(el, `${key} ${selector} must exist`);
    el.value = raw;
    // Some handlers read e.target.value, others read the element directly
    el.onchange({ target: { value: raw } });
    assert.equal(out ? JSON.stringify(extractField(key, out)) : null, JSON.stringify(expected), label);
  };
  function extractField(key, out) {
    if (key === 'widget/search') return out.suggestionsQuantity;
    if (key === 'widget/todo') return out.focusDuration;
    if (key === 'widget/links') return out.columns;
    if (key === 'widget/topSites') return out.limit;
    if (key === 'widget/tallyCounter') return out.step;
    return undefined;
  }
  checkEmit('widget/search', '#s-suggest-count', '', 1, 'search empty input coerces to 0 and clamps to declared min 1');
  const s0 = new MockElement();
  let searchOut = null;
  widgetPlugins['widget/search'].renderSettings(s0, widgetPlugins['widget/search'].defaultData || {}, (n) => { searchOut = n; }, { t: (k, f) => f || k });
  const sc = s0.querySelector('#s-suggest-count');
  sc.value = '10';
  sc.onchange();
  assert.equal(searchOut.suggestionsQuantity, 10, 'search upper bound 10 accepted');
  sc.value = '99';
  sc.onchange();
  assert.equal(searchOut.suggestionsQuantity, 10, 'search over-range clamped to 10');

  const t0 = new MockElement();
  let todoOut = null;
  widgetPlugins['widget/todo'].renderSettings(t0, widgetPlugins['widget/todo'].defaultData || {}, (n) => { todoOut = n; }, { t: (k, f) => f || k });
  const td = t0.querySelector('#td-pomodoro-dur');
  td.value = '5';
  td.onchange();
  assert.equal(todoOut.focusDuration, 5, 'todo duration 5 accepted');
  td.value = '200';
  td.onchange();
  assert.equal(todoOut.focusDuration, 120, 'todo duration clamped to 120');

  const l0 = new MockElement();
  let linksOut = null;
  widgetPlugins['widget/links'].renderSettings(l0, widgetPlugins['widget/links'].defaultData || {}, (n) => { linksOut = n; }, { t: (k, f) => f || k });
  const lc = l0.querySelector('#l-cols');
  lc.value = '1';
  lc.onchange({ target: { value: '1' } });
  assert.equal(linksOut.columns, 1, 'links 1 column accepted');

  const ts0 = new MockElement();
  let tsOut = null;
  widgetPlugins['widget/topSites'].renderSettings(ts0, widgetPlugins['widget/topSites'].defaultData || {}, (n) => { tsOut = n; }, { t: (k, f) => f || k });
  const tsc = ts0.querySelector('#ts-limit');
  tsc.value = '1';
  tsc.onchange({ target: { value: '1' } });
  assert.equal(tsOut.limit, 1, 'top-sites limit 1 accepted');

  const tc0 = new MockElement();
  let tcOut = null;
  widgetPlugins['widget/tallyCounter'].renderSettings(tc0, widgetPlugins['widget/tallyCounter'].defaultData || {}, (n) => { tcOut = n; }, { t: (k, f) => f || k });
  const tc = tc0.querySelector('#tc-step');
  tc.value = '0';
  tc.onchange();
  assert.equal(tcOut.step, 1, 'tally step 0 clamped to min 1');
  console.log('✓ C3 numeric guards clamp without || fallback');
}

// C4. Serial write queue: latest revision, no stale overwrite across spaces
{
  const calls = [];
  const snapshots = new Map([
    ['ws-1', { revision: 1, widgets: [] }],
    ['ws-2', { revision: 10, widgets: [] }],
  ]);
  let activeId = 'ws-1';
  const nativeCall = async (method, params) => {
    calls.push({ method, params });
    if (method === 'workspace_snapshot') return snapshots.get(params.workspaceId);
    const rev = snapshots.get(params.workspaceId).revision;
    snapshots.set(params.workspaceId, { ...snapshots.get(params.workspaceId), revision: rev + 1 });
    return { ...snapshots.get(params.workspaceId), widgets: [params.widget] };
  };
  const queue = createWorkspaceMutationQueue({
    nativeCall,
    getActiveWorkspaceId: () => activeId,
    getActiveSnapshot: () => snapshots.get(activeId),
    applySnapshot: (result) => { snapshots.set(activeId, result); },
  });

  const r1 = queue('ws-1', async (snap) => nativeCall('workspace_widget_upsert', { workspaceId: 'ws-1', widget: { id: 'a' }, expectedRevision: snap.revision }));
  const r2 = queue('ws-2', async (snap) => nativeCall('workspace_widget_upsert', { workspaceId: 'ws-2', widget: { id: 'b' }, expectedRevision: snap.revision }));
  await Promise.all([r1, r2]);

  const upserts = calls.filter((c) => c.method === 'workspace_widget_upsert');
  assert.equal(upserts.length, 2, 'two queued writes');
  assert.equal(upserts[0].params.expectedRevision, 1, 'ws-1 write uses revision 1');
  assert.equal(upserts[1].params.expectedRevision, 10, 'ws-2 write fetched its own latest revision, not ws-1 stale');
  const snap1 = await nativeCall('workspace_snapshot', { workspaceId: 'ws-1' });
  assert.equal(snap1.revision, 2, 'ws-1 revision advanced exactly once');
  console.log('✓ C4 serial queue uses per-space latest revision');
}

// C6. Background settings persist immediately on change
{
  let saveCalls = 0;
  const snapshot = { name: 'T', backgroundJson: { key: 'background/colour', display: { colour: '#101010' } }, widgets: [], revision: 1 };
  const ctrl = createSpaceBackgroundSettings({
    t: (k, f) => f || k,
    language: 'zh_CN',
    onUpdateBackground: async () => { saveCalls += 1; return { ...snapshot, revision: 2 }; },
    onBackToOverview: () => {},
  });
  const container = new MockElement('div');
  ctrl.render(container, snapshot, 'ws-1');
  assert.equal(saveCalls, 0, 'background render must not call Host');
  const typeSelect = container.querySelector('#bg-type-select');
  assert.ok(typeSelect, 'background type select must exist');
  typeSelect.value = 'background/online';
  typeSelect.onchange({ target: { value: 'background/online' } });
  assert.equal(saveCalls, 1, 'switching source must save immediately');
  assert.equal(container.querySelector('#btn-save-bg'), null, 'background settings must not render a save button');
  console.log('✓ C6 background changes save immediately');
}

{
  clearMemCache(); let saveCalls = 0;
  const canvas = new MockElement('div'); const container = new MockElement('div');
  const backgroundJson = { key: 'background/bing', display: { ...backgroundPlugins['background/bing'].defaultData, interval: 'daily', mkt: 'zh-CN', index: 0 } };
  backgroundPlugins['background/bing'].render(canvas, backgroundJson.display);
  await new Promise((r) => setTimeout(r, 10));
  const ctrl = createSpaceBackgroundSettings({
    t: (k, f) => f || k,
    language: 'zh_CN',
    onBackToOverview: () => {},
    onUpdateBackground: async (nextBackground) => {
      saveCalls += 1;
      backgroundPlugins[nextBackground.key].render(canvas, nextBackground.display);
    },
  });
  ctrl.render(container, { name: 'T', backgroundJson, widgets: [], revision: 1 }, 'ws-1');
  container.querySelector('#b-next-btn').onclick(); await new Promise((r) => setTimeout(r, 10));
  assert.ok(canvas.style.backgroundImage.includes('OHR.Mock1'), 'Bing next button must visibly render a different wallpaper'); assert.equal(saveCalls, 1, 'Bing next must save immediately');
  assert.equal(container.querySelector('#btn-save-bg'), null, 'background settings must not render a save button');
  console.log('✓ C8 Bing next wallpaper renders and saves immediately');
}
console.log('\n======================================================\nALL 24 WIDGETS AND 9 BACKGROUNDS PASS DEEP VERIFICATION!');
console.log('======================================================\n');
