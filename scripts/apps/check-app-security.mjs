import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { distributableFiles, ROOT } from '../extension-package.mjs';

const files = distributableFiles();
assert.ok(!files.some((file) => /(?:demo|fund)-ui\.js$|app-module-registry\.js$/.test(file)),
  'business UI and the static app registry must not ship in the extension');
for (const file of files.filter((file) => file.endsWith('.js'))) {
  const source = readFileSync(join(ROOT, 'extension', file), 'utf8');
  assert.ok(!/\bimport\s*(?:\(\s*)?['"]https?:|\bfrom\s+['"]https?:/.test(source), `remote executable import in ${file}`);
}
let tracked;
try {
  if (process.env.NATIVES_FORCE_FILESYSTEM_SCAN === '1') throw new Error('filesystem scan requested');
  tracked = execFileSync('git', ['ls-files', '-z'], { cwd: ROOT, encoding: 'utf8' }).split('\0');
} catch {
  tracked = [];
  const walk = (dir, prefix = '') => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (['.git', 'node_modules', 'target', 'dist', 'tests', 'fixtures'].includes(entry.name)) continue;
      const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(join(dir, entry.name), relative);
      else tracked.push(relative);
    }
  };
  walk(ROOT);
}
assert.ok(!tracked.some((file) => /\.(nap|exe|dll|dylib)$/i.test(file)), 'compiled Native App assets must not be tracked');
console.log('app security and zero Native payload in extension passed');
