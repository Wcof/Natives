import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { distributableFiles, ROOT } from '../extension-package.mjs';
import { APP_UI_MODULES } from '../../extension/app-module-registry.js';

const files = distributableFiles();
for (const path of Object.values(APP_UI_MODULES)) {
  assert.ok(path.startsWith('./apps/') && files.includes(path.slice(2)));
}
for (const file of files.filter((file) => file.endsWith('.js'))) {
  const source = readFileSync(join(ROOT, 'extension', file), 'utf8');
  assert.ok(!/\bimport\s*(?:\(\s*)?['"]https?:|\bfrom\s+['"]https?:/.test(source), `remote executable import in ${file}`);
}
const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: ROOT, encoding: 'utf8' }).split('\0');
assert.ok(!tracked.some((file) => /\.(nap|exe|dll|dylib)$/i.test(file)), 'compiled Native App assets must not be tracked');
console.log('app security and zero Native payload in extension passed');
