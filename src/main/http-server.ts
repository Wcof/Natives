import * as http from 'http';
import * as fs from 'fs';
import * as path from 'path';

const MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules');
const SDK_PATH = path.join(__dirname, '..', 'lib', 'bridge-sdk.js');
let server: http.Server | null = null;
let activePort = 0;

// Token verification (set by bridge-host or main process)
let verifyToken: ((moduleId: string, token: string) => boolean) | null = null;

export function setTokenVerifier(verifier: (moduleId: string, token: string) => boolean): void {
  verifyToken = verifier;
}

// Bridge request handler (set by bridge-host)
let bridgeHandler: ((namespace: string, method: string, moduleId: string, data: unknown) => Promise<unknown>) | null = null;

export function setBridgeHandler(handler: (namespace: string, method: string, moduleId: string, data: unknown) => Promise<unknown>): void {
  bridgeHandler = handler;
}

// ── Host Validation ──

function extractHostname(host: string): string {
  // 处理 IPv6: [::1]:8080 → ::1
  if (host.startsWith('[')) {
    const end = host.indexOf(']');
    return end !== -1 ? host.slice(1, end) : host;
  }
  // 处理 IPv4/域名: localhost:8080 → localhost
  return host.split(':')[0]!;
}

export function validateHost(host: string | undefined): boolean {
  if (!host) return false;
  const hostname = extractHostname(host);
  const allowed = ['localhost', '127.0.0.1', '::1'];
  return allowed.includes(hostname);
}

// ── Origin Validation ──

export function validateOrigin(method: string | undefined, origin: string | undefined): boolean {
  // GET 请求不需要验证
  if (method === 'GET') return true;

  // 没有 origin 头的是非浏览器请求（如 curl）
  if (!origin) return true;

  // 只允许来自 localhost 的请求
  try {
    const url = new URL(origin);
    // URL.hostname 对 IPv6 返回带括号的格式：如 "[::1]"
    return ['localhost', '127.0.0.1', '::1', '[::1]'].includes(url.hostname);
  } catch {
    return false;
  }
}

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
  const cleanPath = filePath.split('?')[0]!;
  const normalized = path.normalize(cleanPath);

  if (normalized.startsWith('..') || normalized.includes('..') || normalized.includes('../') || normalized.includes('..\\')) {
    return null;
  }

  const moduleRoot = path.resolve(MODULES_DIR, moduleId);
  const fullPath = path.resolve(moduleRoot, normalized);

  if (!fullPath.startsWith(moduleRoot + path.sep) && fullPath !== moduleRoot) {
    return null;
  }

  return fullPath;
}

// ── Bridge SDK script ──

let sdkScript: string | null = null;

function getSdkScript(): string {
  if (sdkScript) return sdkScript;
  const candidates = [
    SDK_PATH,
    path.join(__dirname, 'lib', 'bridge-sdk.js'),
    path.join(__dirname, '..', '..', 'src', 'lib', 'bridge-sdk.js'),
  ];
  for (const candidate of candidates) {
    try {
      sdkScript = fs.readFileSync(candidate, 'utf-8');
      return sdkScript;
    } catch {
      // try next candidate
    }
  }
  sdkScript = 'console.error("[Natives SDK] Failed to load bridge-sdk.js");';
  return sdkScript;
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let body = '';
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => resolve(body));
    req.on('error', reject);
  });
}

async function handleRequest(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
  // ── Host Validation ──
  if (!validateHost(req.headers.host)) {
    res.writeHead(403);
    res.end('Forbidden');
    return;
  }

  // ── Origin Validation (POST only) ──
  if (!validateOrigin(req.method, req.headers.origin)) {
    res.writeHead(403);
    res.end('Forbidden');
    return;
  }

  // ── CSP Headers (enhanced) ──
  res.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:* https:; frame-src 'self' https:; frame-ancestors 'none'; form-action 'none'");

  const url = new URL(req.url || '/', `http://localhost:${activePort}`);
  const pathname = decodeURIComponent(url.pathname);

  // ── GET /natives-sdk.js ──
  if (req.method === 'GET' && pathname === '/natives-sdk.js') {
    res.writeHead(200, { 'Content-Type': 'application/javascript' });
    res.end(getSdkScript());
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

  // ── POST /api/bridge/{namespace}/{method} ──
  const bridgeMatch = pathname.match(/^\/api\/bridge\/([^/]+)\/([^/]+)$/);
  if (req.method === 'POST' && bridgeMatch) {
    const namespace = bridgeMatch[1]!;
    const method = bridgeMatch[2]!;

    // Verify Session Token (only if token verifier is configured)
    const token = req.headers['x-session-token'] as string | undefined;
    const moduleId = req.headers['x-module-id'] as string | undefined;

    if (verifyToken) {
      if (!token || !moduleId) {
        res.writeHead(401, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: 'Missing X-Session-Token or X-Module-Id header' }));
        return;
      }
      if (!verifyToken(moduleId, token)) {
        res.writeHead(403, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: 'Invalid session token' }));
        return;
      }
    }

    // Parse body and route to bridge handler
    try {
      const body = await readBody(req);
      const parsed = body ? JSON.parse(body) : {};

      if (bridgeHandler) {
        const result = await bridgeHandler(namespace, method, moduleId || '', parsed);
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify(result));
      } else {
        // Fallback: echo back (compatible with existing tests)
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: true, namespace, method, data: parsed }));
      }
    } catch (err) {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'Invalid JSON or handler error' }));
    }
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

    server = http.createServer((req, res) => {
      handleRequest(req, res).catch((err) => {
        console.error('[HTTP Server] Unhandled error:', err);
        if (!res.headersSent) {
          res.writeHead(500);
          res.end('Internal Server Error');
        }
      });
    });

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
