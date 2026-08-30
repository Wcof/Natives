import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';

const EXTENSION_DIR = new URL('.', import.meta.url).pathname;

function collectJsFiles(dir) {
  const files = [];
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name.startsWith('.')) continue;
    const full = join(dir, name);
    const stat = statSync(full);
    if (stat.isDirectory()) {
      files.push(...collectJsFiles(full));
    } else if (name.endsWith('.js') || name.endsWith('.mjs')) {
      files.push(full);
    }
  }
  return files;
}

const jsFiles = collectJsFiles(EXTENSION_DIR);
assert.ok(jsFiles.length >= 35, `Expected at least 35 JS files in extension, found ${jsFiles.length}`);

for (const file of jsFiles) {
  const res = spawnSync(process.execPath, ['--check', file], { encoding: 'utf8' });
  if (res.status !== 0) {
    console.error(`Syntax check failed for ${file}:\n${res.stderr}`);
    process.exit(1);
  }
}

console.log(`✓ Syntax check passed for all ${jsFiles.length} extension JavaScript files`);
