import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { startServer, stopServer, getPort, validateHost, validateOrigin } from './http-server';

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

  // ── Host Validation ──

  it('should validate localhost host', () => {
    assert.ok(validateHost('localhost'));
    assert.ok(validateHost('localhost:3000'));
    assert.ok(validateHost('127.0.0.1'));
    assert.ok(validateHost('127.0.0.1:8080'));
    assert.ok(validateHost('[::1]'));
    assert.ok(validateHost('[::1]:3000'));
  });

  it('should reject non-localhost host', () => {
    assert.equal(validateHost('example.com'), false);
    assert.equal(validateHost('evil.com:80'), false);
    assert.equal(validateHost('192.168.1.1'), false);
    assert.equal(validateHost(''), false);
  });

  // ── Origin Validation ──

  it('should skip origin validation for GET requests', () => {
    assert.ok(validateOrigin('GET', undefined, undefined));
    assert.ok(validateOrigin('GET', 'http://evil.com', undefined));
  });

  it('should reject POST requests with no origin or referer (CSRF protection)', () => {
    assert.equal(validateOrigin('POST', undefined, undefined), false);
  });

  it('should allow POST requests from localhost origins', () => {
    assert.ok(validateOrigin('POST', 'http://localhost:3000', undefined));
    assert.ok(validateOrigin('POST', 'http://127.0.0.1', undefined));
    assert.ok(validateOrigin('POST', 'http://[::1]:8080', undefined));
  });

  it('should fall back to referer when origin is absent', () => {
    assert.ok(validateOrigin('POST', undefined, 'http://localhost:3000/path'));
    assert.ok(validateOrigin('POST', undefined, 'http://127.0.0.1/path'));
    assert.equal(validateOrigin('POST', undefined, 'http://evil.com/path'), false);
  });

  it('should reject POST requests from non-localhost origins', () => {
    assert.equal(validateOrigin('POST', 'http://evil.com', undefined), false);
    assert.equal(validateOrigin('POST', 'https://attacker.org', undefined), false);
    assert.equal(validateOrigin('POST', 'http://192.168.1.1', undefined), false);
  });

  it('should reject POST requests with invalid origin URLs', () => {
    assert.equal(validateOrigin('POST', 'not-a-url', undefined), false);
  });

  // ── Security Integration Tests ──

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

  it('should include CSP headers with frame-ancestors', async () => {
    const res = await fetch(`http://localhost:${port}/natives-sdk.js`);
    const csp = res.headers.get('content-security-policy');
    assert.ok(csp);
    assert.ok(csp!.includes("default-src 'self'"));
    // Enhanced CSP: add frame-ancestors and form-action restrictions
    assert.ok(csp!.includes("frame-ancestors 'none'"));
    assert.ok(csp!.includes("form-action 'none'"));
  });

  it('should handle POST /api/bridge requests', async () => {
    const res = await fetch(`http://localhost:${port}/api/bridge/db/get`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Origin': 'http://localhost:3000',
      },
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

  // ── Need to use direct HTTP request to override Host header ──

  it('should reject requests with non-localhost Host header', async () => {
    const res = await new Promise<{ status: number; body: string }>((resolve, reject) => {
      const http = require('http');
      const req = http.request({
        hostname: 'localhost',
        port,
        path: '/natives-sdk.js',
        method: 'GET',
        headers: { Host: 'evil.com' },
      }, (resp: any) => {
        let body = '';
        resp.on('data', (chunk: string) => { body += chunk; });
        resp.on('end', () => resolve({ status: resp.statusCode!, body }));
      });
      req.on('error', reject);
      req.end();
    });
    assert.equal(res.status, 403);
    assert.ok(res.body.includes('Forbidden'));
  });
});
