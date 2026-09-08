import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { ROOT } from '../extension-package.mjs';

export const ORIGIN = 'chrome-extension://abcdefghijklmnopabcdefghijklmnop/';
export const CORE_HOST = 'com.natives.file_manager';
export function nativePort(binary, args = []) {
  const child = spawn(binary, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  const pending = new Map();
  let buffer = Buffer.alloc(0), sequence = 0, stderr = '';
  const exit = new Promise((resolveExit) => child.once('exit', (code, signal) => resolveExit({ code, signal })));
  child.stderr.on('data', (chunk) => { stderr = (stderr + chunk).slice(-4096); });
  function fail(error) { for (const item of pending.values()) item.reject(error); pending.clear(); }
  child.on('error', fail);
  child.stdin.on('error', fail);
  child.once('exit', () => fail(new Error('native port closed')));
  child.stdout.on('data', (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 4 && buffer.length >= buffer.readUInt32LE(0) + 4) {
      const length = buffer.readUInt32LE(0);
      const message = JSON.parse(buffer.subarray(4, 4 + length));
      buffer = buffer.subarray(4 + length);
      const item = pending.get(message.id);
      if (item) { pending.delete(message.id); item.resolve(message); }
    }
  });
  async function raw(request) {
    const id = request.id || String(++sequence);
    let timer;
    const promise = new Promise((resolveMessage, reject) => {
      timer = setTimeout(() => { pending.delete(id); reject(new Error('native request timed out')); }, 10_000);
      pending.set(id, { resolve: resolveMessage, reject });
    }).finally(() => clearTimeout(timer));
    const bytes = Buffer.from(JSON.stringify({ ...request, id }));
    const header = Buffer.alloc(4); header.writeUInt32LE(bytes.length);
    child.stdin.write(Buffer.concat([header, bytes]));
    return promise;
  }
  return { child, exit, raw,
    async call(method, params = {}) {
      const response = await raw({ method, params });
      if (!response.ok) throw new Error(response.error);
      return response.result;
    },
    async close() {
      if (child.exitCode !== null || child.signalCode !== null) return 0;
      const start = performance.now();
      child.stdin.end();
      let timer;
      await Promise.race([exit, new Promise((_, reject) => {
        timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('Native EOF budget exceeded: ' + stderr)); }, 2000);
      })]).finally(() => clearTimeout(timer));
      return performance.now() - start;
    },
  };
}

export function appFixture() {
  const root = mkdtempSync(join(tmpdir(), 'natives-app-e2e-'));
  const ports = new Set();
  const binary = join(ROOT, 'target/debug', process.platform === 'win32' ? 'native-file-host.exe' : 'native-file-host');
  return { root, ports,
    connect(host) {
      let port;
      if (host === CORE_HOST) port = nativePort(binary, ['--app-fixture', root, ORIGIN]);
      else {
        if (!/^com\.natives\.app\.[a-z0-9._]+$/.test(host)) throw new Error('invalid fixture host');
        const manifest = JSON.parse(readFileSync(join(root, 'manifests', host + '.json')));
        const runtime = resolve(manifest.path);
        if (!runtime.startsWith(join(root, 'apps') + sep) || manifest.allowed_origins[0] !== ORIGIN) throw new Error('invalid runtime registration');
        port = nativePort(runtime, [ORIGIN]);
      }
      ports.add(port);
      return port;
    },
    async dispose() {
      await Promise.all([...ports].map((port) => port.close()));
      rmSync(root, { recursive: true, force: true });
    },
  };
}
