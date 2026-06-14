import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { startServer, stopServer, getPort } from './http-server';

const TEST_MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules', 'test-module');

describe('HTTPServer', () => {
  let port: number;

  before(async () => {
    // Create test module files
    fs.mkdirSync(TEST_MODULES_DIR, { recursive: true });
    fs.writeFileSync(path.join(TEST_MODULES_DIR, 'index.html'), '<h1>Hello</h1>', 'utf-8');
    fs.writeFileSync(path.join(TEST_MODULES_DIR, 'app.js'), 'console.log("hi");', 'utf-8');

    port = await startServer();
  });

  after(() => {
    stopServer();
    if (fs.existsSync(path.join(process.env.HOME || '~', '.natives'))) {
      fs.rmSync(path.join(process.env.HOME || '~', '.natives'), { recursive: true });
    }
  });

  it('should serve SDK script at /natives-sdk.js', async () => {
    const res = await fetch(`http://localhost:${port}/natives-sdk.js`);
    assert.equal(res.status, 200);
    const text = await res.text();
    assert.ok(text.includes('window.natives'));
  });

  it('should serve static files from modules directory', async () => {
    const res = await fetch(`http://localhost:${port}/modules/test-module/index.html`);
    assert.equal(res.status, 200);
    const text = await res.text();
    assert.ok(text.includes('<h1>Hello</h1>'));
    assert.equal(res.headers.get('content-type'), 'text/html');
  });

  it('should return 404 for non-existent files', async () => {
    const res = await fetch(`http://localhost:${port}/modules/test-module/nonexistent.js`);
    assert.equal(res.status, 404);
  });

  it('should reject path traversal attempts', async () => {
    // URL-encoded path traversal - the URL parser doesn't normalize encoded chars
    const res = await fetch(`http://localhost:${port}/modules/test-module/..%2f..%2f..%2fetc%2fpasswd`);
    assert.equal(res.status, 403);
  });

  it('should include CSP headers', async () => {
    const res = await fetch(`http://localhost:${port}/natives-sdk.js`);
    const csp = res.headers.get('content-security-policy');
    assert.ok(csp);
    assert.ok(csp!.includes("default-src 'self'"));
  });

  it('should handle POST /api/bridge requests', async () => {
    const res = await fetch(`http://localhost:${port}/api/bridge/db/get`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ key: 'test' }),
    });
    assert.equal(res.status, 200);
    const data = await res.json();
    assert.equal(data.ok, true);
    assert.equal(data.namespace, 'db');
    assert.equal(data.method, 'get');
  });

  it('should auto-select an available port', () => {
    const p = getPort();
    assert.ok(p > 0);
    assert.ok(p < 65536);
  });
});
