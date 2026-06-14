import * as http from 'http';
import * as fs from 'fs';
import * as path from 'path';

const MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules');
let server: http.Server | null = null;
let activePort = 0;

function getMimeType(ext: string): string {
  const mime: Record<string, string> = {
    '.html': 'text/html',
    '.js': 'application/javascript',
    '.css': 'text/css',
    '.json': 'application/json',
    '.png': 'image/png',
    '.svg': 'image/svg+xml',
    '.ico': 'image/x-icon',
  };
  return mime[ext] || 'application/octet-stream';
}

function sanitizePath(moduleId: string, filePath: string): string | null {
  // Strip query strings
  const cleanPath = filePath.split('?')[0]!;

  // Normalize and resolve within module directory
  const normalized = path.normalize(cleanPath);

  // Check for path traversal patterns
  if (normalized.startsWith('..') || normalized.includes('..') || normalized.includes('../') || normalized.includes('..\\')) {
    return null;
  }

  const moduleRoot = path.resolve(MODULES_DIR, moduleId);
  const fullPath = path.resolve(moduleRoot, normalized);

  // Ensure resolved path is within the module directory
  if (!fullPath.startsWith(moduleRoot + path.sep) && fullPath !== moduleRoot) {
    return null;
  }

  return fullPath;
}

// ── Bridge SDK script content ──
const SDK_SCRIPT = `
(function() {
  'use strict';
  const port = document.currentScript ? new URL(document.currentScript.src).port : '';

  const pending = {};
  let msgId = 0;

  function sendRequest(namespace, method, body) {
    return new Promise((resolve, reject) => {
      const id = ++msgId;
      pending[id] = { resolve, reject };
      const xhr = new XMLHttpRequest();
      xhr.open('POST', '/api/bridge/' + namespace + '/' + method, true);
      xhr.setRequestHeader('Content-Type', 'application/json');
      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          resolve(JSON.parse(xhr.responseText));
        } else {
          reject(new Error(xhr.statusText));
        }
      };
      xhr.onerror = () => reject(new Error('Network error'));
      xhr.send(JSON.stringify(body));
    });
  }

  window.natives = window.natives || {};
  window.natives.db = {
    get: (key) => sendRequest('db', 'get', { key }),
    set: (key, value) => sendRequest('db', 'set', { key, value }),
    delete: (key) => sendRequest('db', 'delete', { key }),
    list: (prefix) => sendRequest('db', 'list', { prefix }),
  };
  window.natives.meta = { moduleId: '', version: '', nativesVersion: '0.1.0' };
  window.natives.lifecycle = {
    ready: () => window.parent.postMessage({ type: 'lifecycle:ready' }, '*'),
    onUnload: (cb) => { window._nativesOnUnload = cb; },
    onHeartbeat: (cb) => { window._nativesHeartbeat = setInterval(cb, 5000); },
    error: (info) => window.parent.postMessage({ type: 'lifecycle:error', info }, '*'),
  };
})();
`;

function handleRequest(req: http.IncomingMessage, res: http.ServerResponse): void {
  // ── CSP Headers ──
  res.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:*");

  const url = new URL(req.url || '/', `http://localhost:${activePort}`);
  const pathname = decodeURIComponent(url.pathname);

  // ── GET /natives-sdk.js ──
  if (req.method === 'GET' && pathname === '/natives-sdk.js') {
    res.writeHead(200, { 'Content-Type': 'application/javascript' });
    res.end(SDK_SCRIPT);
    return;
  }

  // ── GET /modules/{moduleId}/{path} ──
  const moduleMatch = pathname.match(/^\/modules\/([^/]+)\/(.+)$/);
  if (req.method === 'GET' && moduleMatch) {
    const moduleId = moduleMatch[1]!;
    const filePath = moduleMatch[2]!;
    const safePath = sanitizePath(moduleId, filePath);

    if (!safePath) {
      res.writeHead(403);
      res.end('Forbidden');
      return;
    }

    if (!fs.existsSync(safePath)) {
      res.writeHead(404);
      res.end('Not Found');
      return;
    }

    const ext = path.extname(safePath);
    res.writeHead(200, { 'Content-Type': getMimeType(ext) });
    res.end(fs.readFileSync(safePath));
    return;
  }

  // ── POST /api/bridge/{namespace}/{method} ── (placeholder)
  const bridgeMatch = pathname.match(/^\/api\/bridge\/([^/]+)\/([^/]+)$/);
  if (req.method === 'POST' && bridgeMatch) {
    // Just echo back for now
    let body = '';
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => {
      try {
        const parsed = JSON.parse(body);
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: true, namespace: bridgeMatch[1], method: bridgeMatch[2], data: parsed }));
      } catch {
        res.writeHead(400);
        res.end('Invalid JSON');
      }
    });
    return;
  }

  // ── 404 ──
  res.writeHead(404);
  res.end('Not Found');
}

export function startServer(port?: number): Promise<number> {
  return new Promise((resolve, reject) => {
    if (server) {
      resolve(activePort);
      return;
    }

    server = http.createServer(handleRequest);

    const listenPort = port || 0;
    server.listen(listenPort, () => {
      const addr = server!.address();
      if (addr && typeof addr === 'object') {
        activePort = addr.port;
        resolve(activePort);
      } else {
        reject(new Error('Failed to get server address'));
      }
    });

    server.on('error', reject);
  });
}

export function stopServer(): void {
  if (server) {
    server.close();
    server = null;
    activePort = 0;
  }
}

export function getPort(): number {
  return activePort;
}
