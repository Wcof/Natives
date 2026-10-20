#!/usr/bin/env node
// One-command development launcher: build/register the extension host, start the
// shared Core Web service, then open the Chrome extension and the local Web.
// This is a dev convenience; the installed product uses the single launcher app.
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { homedir, platform } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const noOpen = process.argv.includes('--no-open');
const webOnly = process.argv.includes('--web-only');
const extensionOnly = process.argv.includes('--extension-only');
const host = join(root, 'target', 'debug', platform() === 'win32' ? 'native-file-host.exe' : 'native-file-host');
const dev = join(root, 'extension', 'dev.mjs');
const devExtension = join(root, 'dist', 'dev-extension');
const launch = join(root, 'extension', 'launch-natives.mjs');
const children = new Set();
let shuttingDown = false;

function run(args, options = {}) {
  const child = spawn(process.execPath, args, { cwd: root, stdio: ['ignore', 'pipe', 'pipe'], ...options });
  children.add(child);
  child.once('exit', () => children.delete(child));
  return child;
}
function signal(child, sig) {
  if (process.platform !== 'win32' && child.pid) {
    try { process.kill(-child.pid, sig); return; } catch {}
  }
  try { child.kill(sig); } catch {}
}
function runExecutable(file, args, options = {}) {
  const child = spawn(file, args, { cwd: root, stdio: ['ignore', 'pipe', 'pipe'], ...options });
  children.add(child);
  child.once('exit', () => children.delete(child));
  return child;
}
function pipe(child) {
  child.stdout.on('data', data => process.stdout.write(data));
  child.stderr.on('data', data => process.stderr.write(data));
}
function waitFor(child, marker, timeoutMs) {
  return new Promise((resolvePromise, reject) => {
    let buffer = '';
    const timer = setTimeout(() => reject(new Error(`timeout waiting for ${marker}`)), timeoutMs);
    const onData = data => {
      buffer += data.toString();
      if (buffer.includes(marker)) {
        clearTimeout(timer);
        child.stdout.off('data', onData);
        resolvePromise();
      }
    };
    child.stdout.on('data', onData);
    child.once('exit', code => {
      clearTimeout(timer);
      reject(new Error(`process exited (${code}) before ${marker}`));
    });
  });
}
async function waitHealth(timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    try {
      const response = await fetch('http://127.0.0.1:8771/healthz');
      if (response.ok) return true;
    } catch {}
    await new Promise(resolvePromise => setTimeout(resolvePromise, 150));
  }
  return false;
}
async function stop() {
  if (shuttingDown) return;
  shuttingDown = true;
  for (const child of [...children]) {
    signal(child, 'SIGTERM');
  }
  await new Promise(resolvePromise => setTimeout(resolvePromise, 2500));
  for (const child of [...children]) {
    signal(child, 'SIGKILL');
  }
}
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => {
  await stop();
  process.exit(0);
});

async function main() {
if (!existsSync(dev)) throw new Error('missing extension/dev.mjs');
const devChild = run([dev]);
pipe(devChild);
console.log('Natives: building Host, extension and product source…');
await waitFor(devChild, 'Natives dev ready.', 180_000);
const devManifest = JSON.parse(readFileSync(join(devExtension, 'manifest.json'), 'utf8'));
const digest = createHash('sha256').update(Buffer.from(devManifest.key, 'base64')).digest('hex').slice(0, 32);
const expectedId = [...digest].map(c => String.fromCharCode(97 + parseInt(c, 16))).join('');
console.log(`Natives: dev extension and Native Hosts ready. Expected extension ID: ${expectedId}`);

// The dev command intentionally does not start a browser or the Web service.
// Start the same Core service used by the product, then open both entries.
if (!(await waitHealth(1500))) {
  const service = spawn(host, ['--service'], { cwd: root, stdio: ['ignore', 'inherit', 'inherit'], detached: process.platform !== 'win32' });
  children.add(service);
  service.once('exit', () => children.delete(service));
  if (!(await waitHealth(15_000))) throw new Error('local Web service did not become ready on 127.0.0.1:8771');
}
console.log('Natives: shared Web service ready at http://127.0.0.1:8771');
if (!noOpen && !extensionOnly) {
  const web = runExecutable(host, ['--launcher-web']);
  pipe(web);
}
if (!noOpen && !webOnly) {
  const extension = run([launch]);
  pipe(extension);
}
const ready = [`Natives ready.${webOnly ? ' (Web only)' : extensionOnly ? ' (Extension only)' : ''}`];
if (!webOnly) ready.push(`Extension: ${devExtension}`, `Expected extension ID: ${expectedId}`);
if (!extensionOnly) ready.push('Web:       http://127.0.0.1:8771');
if (!webOnly) ready.push('First run only: load dist/dev-extension in chrome://extensions if Chrome has not loaded it yet.');
ready.push('Press Ctrl+C to stop the dev Host and the shared Web service.');
console.log(ready.join('\n'));
await new Promise(() => {});
}

main().catch(async error => {
  console.error(`Natives start failed: ${error.message}`);
  await stop();
  process.exit(1);
});
