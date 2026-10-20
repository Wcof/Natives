// DES T01 / A37（P00 对照）：投资模块外壳在真实 Chrome 中按 archive/volt
// 消费单源组合主题。装配方式与 home-browser.test.mjs 一致（src partials +
// 组合后 theme.css），数据为合成夹具，不触真实账本。
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '../..');
const src = join => resolve(ROOT, 'modules/fund/ui/src', join);
const artifacts = resolve(ROOT, '.tmp/des-t01/fund-shell');
mkdirSync(artifacts, { recursive: true });

const themeCss = readFileSync(resolve(ROOT, 'modules/fund/ui/dist/styles/theme.css'), 'utf8'); // 构建期组合产物
const css = [themeCss,
  readFileSync(src('styles/layout.css')), readFileSync(src('styles/panel.css')),
  readFileSync(src('styles/home.css')), readFileSync(src('styles/investment.css'))].join('\n');
const header = readFileSync(src('partials/header.html'));

const EXPECTED = {
  archive: { bg: 'rgb(245, 246, 248)', text: 'rgb(32, 39, 51)', accent: '#345780', up: '#b13642', down: '#217052' },
  volt: { bg: 'rgb(11, 12, 10)', text: 'rgb(242, 242, 234)', accent: '#cdf24b', up: '#ff9a9f', down: '#8cd6b3' },
};

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const results = [];
try {
  for (const theme of ['archive', 'volt']) {
    for (const width of [1280, 640]) {
      const page = await browser.newPage({ viewport: { width, height: 900 } });
      const errors = [];
      page.on('pageerror', (e) => errors.push(String(e.message || e)));
      await page.setContent(`<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><style>${css}</style></head><body><div class="app-container">${header}</div></body></html>`);
      await page.evaluate((t) => { document.documentElement.dataset.theme = t; }, theme);
      await page.waitForTimeout(100);
      const checks = await page.evaluate(() => {
        const cs = getComputedStyle(document.documentElement);
        const body = getComputedStyle(document.body);
        const up = document.querySelector('.up, .ticker-val');
        return {
          bg: body.backgroundColor,
          color: body.color,
          accent: cs.getPropertyValue('--natives-accent').trim(),
          up: cs.getPropertyValue('--color-up').trim(),
          down: cs.getPropertyValue('--color-down').trim(),
          upColor: up ? getComputedStyle(up).color : null,
          overflow: document.documentElement.scrollWidth - window.innerWidth,
        };
      });
      const exp = EXPECTED[theme];
      const pass = checks.bg === exp.bg && checks.color === exp.text && checks.accent === exp.accent
        && checks.up === exp.up && checks.down === exp.down && checks.overflow <= 1 && errors.length === 0;
      void checks.upColor;
      const shot = resolve(artifacts, `fund-shell-${theme}-${width}.png`);
      await page.screenshot({ path: shot });
      results.push({ theme, width, pass, checks, errors: errors.slice(0, 3), shot });
      await page.close();
    }
  }
} finally {
  await browser.close();
}
writeFileSync(resolve(artifacts, 'results.json'), JSON.stringify(results, null, 2) + '\n');
const failed = results.filter((r) => !r.pass);
console.log(`fund shell theme check: ${results.length - failed.length}/${results.length} passed`);
for (const f of failed) console.log(`FAIL ${f.theme} ${f.width}: ${JSON.stringify({ checks: f.checks, errors: f.errors })}`);
if (failed.length) process.exit(1);
