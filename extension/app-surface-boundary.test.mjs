import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

assert.ok(!existsSync(new URL('./apps/demo-ui.js', import.meta.url)));
assert.ok(!existsSync(new URL('./apps/fund-ui.js', import.meta.url)));
assert.ok(!existsSync(new URL('./app-module-registry.js', import.meta.url)));
const shell = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
assert.ok(shell.includes('iframe'));
assert.ok(!shell.includes('import('));
console.log('built-in modules: no extension-bundled business UI or static registry');
