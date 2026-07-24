import { existsSync, readFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';

const budget = 350 * 1024;
const manifestPath = '.next/app-build-manifest.json';
const routes = ['/page', '/modules/page', '/files/page'];

if (!existsSync(manifestPath)) {
  console.error(`Missing ${manifestPath}; run npm run build first.`);
  process.exit(1);
}

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
const pages = manifest.pages ?? {};
const layout = pages['/layout'] ?? [];
let failed = false;

for (const route of routes) {
  const files = [...new Set([...layout, ...(pages[route] ?? [])])].filter((file) => file.endsWith('.js'));
  const bytes = files.reduce((total, file) => {
    const path = `.next/${file}`;
    return total + (existsSync(path) ? gzipSync(readFileSync(path)).byteLength : 0);
  }, 0);
  const status = bytes <= budget ? 'ok' : 'over';
  console.log(`${route}: ${(bytes / 1024).toFixed(1)} KB gzip (${status}, budget ${(budget / 1024).toFixed(0)} KB)`);
  failed ||= bytes > budget;
}

if (failed) process.exit(1);
