import { chromium } from 'playwright-core';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const FFMPEG = process.env.HOME + '/.workbuddy/tools/node_modules/ffmpeg-static/ffmpeg';
const HTML = path.resolve(process.argv[2] || 'natives-local.html');
const FPS = Number(process.env.FPS || 60);
const DUR = Number(process.env.DUR || 60);
const WORKERS = Number(process.env.WORKERS || 5);
const OUT = process.argv[3] || HTML.replace(/\.html$/, '.mp4');
const TMP = path.join(path.dirname(HTML), '.frames-' + process.pid);

const TOTAL = Math.round(FPS * DUR);
fs.mkdirSync(TMP, { recursive: true });

const t0 = Date.now();
const browser = await chromium.launch({
  executablePath: CHROME,
  headless: true,
  args: ['--force-color-profile=srgb', '--disable-lcd-text', '--allow-file-access-from-files',
         '--hide-scrollbars', '--disable-background-timer-throttling', '--disable-renderer-backgrounding']
});

async function worker(w) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  await page.goto('file://' + HTML, { waitUntil: 'load' });
  await page.waitForFunction(() => window.__ready === true, { timeout: 20000 });
  await page.evaluate(() => { window.__recording = true; });

  const from = Math.floor(TOTAL * w / WORKERS);
  const to = Math.floor(TOTAL * (w + 1) / WORKERS);
  for (let f = from; f < to; f++) {
    const t = f / FPS;
    await page.evaluate(tt => window.__seek(tt), t);
    await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))));
    await page.screenshot({ path: path.join(TMP, String(f).padStart(5, '0') + '.png'), type: 'png' });
    if (f % 300 === 0) {
      const pct = ((Date.now() - t0) / 1000).toFixed(0);
      console.log(`w${w} frame ${f}/${TOTAL}  ${pct}s`);
    }
  }
  await page.close();
}

await Promise.all(Array.from({ length: WORKERS }, (_, i) => worker(i)));
await browser.close();
console.log('captured', TOTAL, 'frames in', ((Date.now() - t0) / 1000).toFixed(1), 's');

const r = spawnSync(FFMPEG, [
  '-y', '-framerate', String(FPS), '-i', path.join(TMP, '%05d.png'),
  '-c:v', 'libx264', '-preset', 'slow', '-crf', '16', '-pix_fmt', 'yuv420p',
  '-profile:v', 'high', '-level', '4.2', '-movflags', '+faststart', OUT
], { stdio: 'inherit' });
if (r.status !== 0) { console.error('ffmpeg failed'); process.exit(1); }
console.log('wrote', OUT, (fs.statSync(OUT).size / 1048576).toFixed(2), 'MB');
