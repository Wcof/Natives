import { chromium } from 'playwright-core';
import path from 'node:path';

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const FILE = process.argv[2];
const times = process.argv.slice(3).map(Number);
const outDir = path.dirname(FILE);

const browser = await chromium.launch({
  executablePath: CHROME,
  headless: true,
  args: ['--force-color-profile=srgb', '--disable-lcd-text', '--allow-file-access-from-files', '--hide-scrollbars']
});
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
const errors = [];
page.on('pageerror', e => errors.push('PAGEERROR ' + e.message));
page.on('console', m => { if (m.type() === 'error') errors.push('CONSOLE ' + m.text()); });

await page.goto('file://' + FILE, { waitUntil: 'load' });
await page.waitForFunction(() => window.__ready === true, { timeout: 15000 }).catch(() => errors.push('no __ready'));
await page.waitForTimeout(500);

for (const t of times) {
  await page.evaluate(tt => window.__seek(tt), t);
  await page.waitForTimeout(90);
  const name = `kf-${String(t.toFixed(2)).replace('.', '_')}.png`;
  await page.screenshot({ path: path.join(outDir, name) });
  console.log('shot', name);
}
const box = await page.evaluate(() => {
  const r = document.getElementById('stage').getBoundingClientRect();
  return { w: r.width, h: r.height, left: r.left, top: r.top };
});
console.log('stage', JSON.stringify(box));
console.log('errors', JSON.stringify(errors));
await browser.close();
