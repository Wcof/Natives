// DES A37/A01（T01 批次）：五个宿主／辅助页在真实 Chrome 中的基础主题矩阵。
// 5 页 × 2 主题 × 2 语言 × 4 宽度 = 80 组合。
// 每组校验：页面无脚本错误、基础角色 computed style 与单源 theme.css 一致
// （body 底色 = --natives-bg、正文色 = --natives-text-primary、accent 可解析）、
// 文档不横向溢出，并保存截图。文件数据/Native 业务不在此验证（A38 专项）。
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join, dirname, extname, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '../..');
const extRoot = join(ROOT, 'extension');
const artifacts = join(ROOT, '.tmp/des-t01/host-matrix');
mkdirSync(artifacts, { recursive: true });

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json', '.html': 'text/html', '.css': 'text/css', '.svg': 'image/svg+xml' };
const server = createServer((req, res) => {
  const urlPath = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  let filePath = join(extRoot, urlPath);
  try {
    const body = readFileSync(filePath);
    res.writeHead(200, { 'content-type': MIME[extname(filePath)] || 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404); res.end('not found');
  }
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const baseUrl = `http://127.0.0.1:${server.address().port}`;

const locales = Object.fromEntries(['zh_CN', 'en'].map((lang) =>
  [lang, JSON.parse(readFileSync(join(extRoot, `_locales/${lang}/messages.json`), 'utf8'))]));

const PAGES = [
  { id: 'H01', file: 'files.html', probe: '#app-sidebar' },
  { id: 'H02', file: 'apps.html', probe: '.apps-nav' },
  { id: 'H03', file: 'app.html', probe: '.app-shell-content, body' },
  { id: 'H04', file: 'space.html', probe: '#app-sidebar, .space-shell, body' },
  { id: 'H05', file: 'screenshot.html', probe: '.screenshot-toolbar' },
];
const WIDTHS = [320, 640, 1280, 1440];
const THEMES = ['archive', 'volt'];
const LANGS = ['zh_CN', 'en'];

// 期望值直接从单源 theme.css 解析（不是复制设计表）。
function parseBaseRoles() {
  const css = readFileSync(join(extRoot, 'theme.css'), 'utf8');
  const blockFor = (theme) => {
    if (theme === 'archive') return css.slice(css.indexOf(':root {'), css.indexOf(':root[data-theme="volt"]'));
    return css.slice(css.indexOf(':root[data-theme="volt"]'));
  };
  const pick = (block, role) => {
    const m = block.match(new RegExp(`${role.replace(/-/g, '\\-')}:\\s*([^;]+);`));
    return m ? m[1].trim() : null;
  };
  return Object.fromEntries(THEMES.map((theme) => [theme, {
    bg: pick(blockFor(theme), '--natives-bg'),
    text: pick(blockFor(theme), '--natives-text-primary'),
    accent: pick(blockFor(theme), '--natives-accent'),
  }]));
}
const EXPECTED = parseBaseRoles();

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const results = [];
try {
  for (const { id, file, probe } of PAGES) {
    for (const theme of THEMES) {
      for (const lang of LANGS) {
        for (const width of WIDTHS) {
          const page = await browser.newPage({ viewport: { width, height: 900 } });
          const errors = [];
          page.on('pageerror', (e) => errors.push(String(e.message || e)));
          const messages = locales[lang];
          await page.addInitScript(({ theme, lang, messages }) => {
            const store = { 'natives-theme': theme, 'natives-language': lang };
            window.chrome = {
              runtime: {
                getURL: (p) => new URL(p, location.href).href,
                connectNative: () => ({ onMessage: { addListener() {} }, onDisconnect: { addListener() {} }, postMessage() {}, disconnect() {} }),
                sendMessage: (...args) => { const cb = args[args.length - 1]; if (typeof cb === 'function') setTimeout(() => cb(undefined), 0); },
              },
              storage: {
                local: { get: async (d) => (typeof d === 'object' && d ? { ...d, ...store } : { [d]: store[d] }), set: async () => {} },
                onChanged: { addListener() {} },
              },
              i18n: { getMessage: (k) => messages[k]?.message ?? k },
              tabs: { create() {}, query: async () => [], update() {} },
            };
          }, { theme, lang, messages });
          await page.goto(`${baseUrl}/${file}`, { waitUntil: 'load' });
          await page.waitForTimeout(400);
          await page.locator(probe).first().waitFor({ state: 'attached', timeout: 4000 }).catch(() => {});
          const checks = await page.evaluate(() => {
            const cs = getComputedStyle(document.documentElement);
            const body = getComputedStyle(document.body);
            return {
              dataTheme: document.documentElement.dataset.theme || '(unset)',
              bg: body.backgroundColor,
              color: body.color,
              accent: cs.getPropertyValue('--natives-accent').trim(),
              surface: cs.getPropertyValue('--natives-surface').trim(),
              colorScheme: cs.colorScheme,
              overflow: document.documentElement.scrollWidth - window.innerWidth,
            };
          });
          const exp = EXPECTED[theme];
          const toRgba = (hex) => {
            const m = hex.match(/^#([0-9a-fA-F]{6})$/);
            if (!m) return hex;
            const n = parseInt(m[1], 16);
            return `rgb(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255})`;
          };
          const ok = {
            themeAttr: checks.dataTheme === theme,
            bg: checks.bg === toRgba(exp.bg),
            text: checks.color === toRgba(exp.text),
            accentResolved: checks.accent === exp.accent,
            colorScheme: checks.colorScheme === (theme === 'volt' ? 'dark' : 'light'),
            noOverflow: checks.overflow <= 1,
            noFatalErrors: errors.length === 0,
          };
          const pass = Object.values(ok).every(Boolean);
          const shot = join(artifacts, `${id}-${theme}-${lang}-${width}.png`);
          await page.screenshot({ path: shot, fullPage: false });
          results.push({ id, file, theme, lang, width, pass, ok, checks, errors: errors.slice(0, 3), shot: pass ? shot : `${shot} (failed)` });
          await page.close();
        }
      }
    }
  }
} finally {
  await browser.close();
  server.close();
}
writeFileSync(join(artifacts, 'results.json'), JSON.stringify(results, null, 2) + '\n');
const failed = results.filter((r) => !r.pass);
console.log(`host theme matrix: ${results.length - failed.length}/${results.length} passed`);
for (const f of failed) console.log(`FAIL ${f.id} ${f.theme} ${f.lang} ${f.width}: ${JSON.stringify({ ok: f.ok, checks: f.checks, errors: f.errors })}`);
if (failed.length) process.exit(1);
