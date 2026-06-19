import * as http from 'http';
import * as fs from 'fs';
import * as path from 'path';
import * as crypto from 'crypto';
import * as os from 'os';
import { execFile } from 'child_process';
import { streamFile } from './file-manager';

// ── HEIC/HEIF transcoding (macOS sips) ──

const HEIC_EXT = new Set(['.heic', '.heif']);
const THUMB_DIR = path.join(os.homedir(), '.natives', 'thumbs');
const thumbInflight = new Map<string, Promise<string | null>>();

function heicCacheKey(file: string, mtimeMs: number): string {
  return crypto.createHash('md5').update(`${file}:${mtimeMs}`).digest('hex');
}

async function pruneThumbs(): Promise<void> {
  try {
    const files = await fs.promises.readdir(THUMB_DIR);
    let total = 0;
    const entries: Array<{ name: string; size: number; atime: number }> = [];
    for (const f of files) {
      if (f === 'meta.json') continue;
      try {
        const s = await fs.promises.stat(path.join(THUMB_DIR, f));
        total += s.size;
        entries.push({ name: f, size: s.size, atime: s.atimeMs });
      } catch { /* ignore */ }
    }
    if (total <= 400 * 1024 * 1024) return;
    entries.sort((a, b) => a.atime - b.atime);
    for (const e of entries) {
      if (total <= 380 * 1024 * 1024) break;
      try { await fs.promises.unlink(path.join(THUMB_DIR, e.name)); } catch { /* */ }
      total -= e.size;
    }
  } catch { /* no cache dir yet */ }
}

/**
 * Transcode HEIC/HEIF → JPEG via macOS sips, with caching.
 * Returns the cached JPEG path, or null on failure.
 */
async function serveHeicAsJpegPath(file: string, stat: fs.Stats): Promise<string | null> {
  const key = heicCacheKey(file, stat.mtimeMs);
  const cacheFile = path.join(THUMB_DIR, `${key}.heic.jpg`);

  // Cache hit
  try {
    await fs.promises.access(cacheFile);
    return cacheFile;
  } catch { /* miss */ }

  // Deduplicate concurrent requests
  const inflight = thumbInflight.get(key);
  if (inflight) return inflight;

  const promise = (async () => {
    await fs.promises.mkdir(THUMB_DIR, { recursive: true });
    try {
      await new Promise<void>((resolve, reject) => {
        const proc = execFile('sips', ['-s', 'format', 'jpeg', file, '--out', cacheFile], { timeout: 15000 });
        proc.on('error', reject);
        proc.on('close', (code) => code === 0 ? resolve() : reject(new Error(`sips exit ${code}`)));
      });
      await pruneThumbs();
      return cacheFile;
    } catch {
      return null;
    }
  })();

  thumbInflight.set(key, promise);
  try {
    return await promise;
  } finally {
    thumbInflight.delete(key);
  }
}

/** Check if a file extension is HEIC/HEIF */
function isHeic(filePath: string): boolean {
  return HEIC_EXT.has(path.extname(filePath).toLowerCase());
}

function getModulesDir() {
  if (process.env.NATIVES_DB_DIR) {
    return path.join(process.env.NATIVES_DB_DIR, 'modules');
  }
  return path.join(process.env.HOME || '~', '.natives', 'modules');
}
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

// ── Origin Validation (TASK-004: CSRF protection) ──

export function validateOrigin(method: string | undefined, origin: string | undefined, referer: string | undefined): boolean {
  // GET 请求不需要验证
  if (method === 'GET') return true;

  // 优先检查 Origin 头
  if (origin) {
    try {
      const url = new URL(origin);
      return ['localhost', '127.0.0.1', '::1', '[::1]'].includes(url.hostname);
    } catch {
      return false;
    }
  }

  // 没有 Origin 时检查 Referer（非浏览器客户端如 curl）
  if (referer) {
    try {
      const url = new URL(referer);
      return ['localhost', '127.0.0.1', '::1', '[::1]'].includes(url.hostname);
    } catch {
      return false;
    }
  }

  // 既无 Origin 也无 Referer → 拒绝（防御 CSRF / 任意客户端）
  return false;
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

  const moduleRoot = path.resolve(getModulesDir(), moduleId);
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
  // ── Host Validation (TASK-003) ──
  if (!validateHost(req.headers.host)) {
    res.writeHead(403);
    res.end('Forbidden');
    return;
  }

  // ── Origin Validation — POST only, checks Origin then Referer (TASK-004) ──
  if (!validateOrigin(req.method, req.headers.origin, req.headers.referer)) {
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

  // ── GET /api/fs/raw?path=... — 文件预览（HEIC 自动转码）──
  if (req.method === 'GET' && pathname === '/api/fs/raw') {
    const filePath = url.searchParams.get('path');
    if (!filePath) {
      res.writeHead(400);
      res.end('Missing path parameter');
      return;
    }

    // 安全校验：拒绝空字节和路径穿越
    if (filePath.includes('\0') || filePath.includes('..')) {
      res.writeHead(403);
      res.end('Forbidden');
      return;
    }

    try {
      const stat = await fs.promises.stat(filePath);
      if (!stat.isFile()) {
        res.writeHead(400);
        res.end('Path is a directory');
        return;
      }

      // HEIC/HEIF → 透明转码为 JPEG（markdown ![](x.heic) 可直接显示）
      if (isHeic(filePath)) {
        const jpegPath = await serveHeicAsJpegPath(filePath, stat);
        if (jpegPath) {
          const jpegStat = await fs.promises.stat(jpegPath);
          const stream = fs.createReadStream(jpegPath);
          res.writeHead(200, {
            'Content-Type': 'image/jpeg',
            'Content-Length': String(jpegStat.size),
            'Cache-Control': 'public, max-age=604800',
          });
          stream.pipe(res);
          stream.on('error', () => { if (!res.headersSent) { res.writeHead(500); res.end('Transcode error'); } });
          return;
        }
        // sips 失败 → 415
        res.writeHead(415);
        res.end('HEIC transcode failed');
        return;
      }

      const rangeHeader = req.headers.range;
      let range: { start: number; end?: number } | undefined;

      if (rangeHeader) {
        const match = rangeHeader.match(/bytes=(\d+)-(\d*)/);
        if (match) {
          range = { start: parseInt(match[1]!, 10) };
          if (match[2] !== '') range.end = parseInt(match[2]!, 10);
        }
      }

      const result = await streamFile(filePath, range);

      res.writeHead(range ? 206 : 200, {
        'Content-Type': result.contentType,
        'Accept-Ranges': 'bytes',
        'Content-Length': range
          ? String(result.totalSize - (range.start || 0))
          : String(result.totalSize),
        'Cache-Control': 'no-cache',
        ...(result.contentRange ? { 'Content-Range': result.contentRange } : {}),
      });

      result.stream.pipe(res);
      result.stream.on('error', () => {
        if (!res.headersSent) {
          res.writeHead(500);
          res.end('Internal Server Error');
        }
      });
    } catch (err: any) {
      if (err?.code === 'ENOENT') {
        res.writeHead(404);
        res.end('File not found');
      } else if (err?.code === 'EISDIR') {
        res.writeHead(400);
        res.end('Path is a directory');
      } else {
        res.writeHead(500);
        res.end('Internal Server Error');
      }
    }
    return;
  }

  // ── GET /api/fs/thumb?path=...&w=... — 缩略图（含 HEIC）──
  if (req.method === 'GET' && pathname === '/api/fs/thumb') {
    const filePath = url.searchParams.get('path');
    const width = parseInt(url.searchParams.get('w') || '320', 10);
    if (!filePath) {
      res.writeHead(400);
      res.end('Missing path parameter');
      return;
    }
    if (filePath.includes('\0') || filePath.includes('..')) {
      res.writeHead(403);
      res.end('Forbidden');
      return;
    }
    try {
      const { generateThumb } = await import('./thumbnail');
      const result = await generateThumb(filePath, width);
      if (result) {
        res.writeHead(200, {
          'Content-Type': result.contentType,
          'Content-Length': String(result.buffer.length),
          'Cache-Control': 'public, max-age=604800',
        });
        res.end(result.buffer);
      } else {
        res.writeHead(415);
        res.end('Cannot generate thumbnail');
      }
    } catch {
      res.writeHead(500);
      res.end('Thumbnail generation failed');
    }
    return;
  }

  // ── POST /api/fs/copy — 复制文件到目标目录（拖拽 from Finder）──
  if (req.method === 'POST' && pathname === '/api/fs/copy') {
    const src = url.searchParams.get('src');
    const dir = url.searchParams.get('dir');
    if (!src || !dir) {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'Missing src or dir parameter' }));
      return;
    }
    if (src.includes('\0') || dir.includes('\0') || dir.includes('..')) {
      res.writeHead(403);
      res.end('Forbidden');
      return;
    }
    try {
      await fs.promises.mkdir(dir, { recursive: true });
      const name = path.basename(src);
      const target = path.join(dir, name);
      await fs.promises.copyFile(src, target);
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ ok: true, path: target }));
    } catch (err: any) {
      res.writeHead(500, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: err?.message || 'Copy failed' }));
    }
    return;
  }

  // ── POST /api/fs/save — 保存文件（拖拽存盘）──
  if (req.method === 'POST' && pathname === '/api/fs/save') {
    const dir = url.searchParams.get('dir');
    const name = url.searchParams.get('name');
    if (!dir || !name) {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'Missing dir or name parameter' }));
      return;
    }
    if (dir.includes('\0') || dir.includes('..') || name.includes('\0') || name.includes('/') || name.includes('\\')) {
      res.writeHead(403);
      res.end('Forbidden');
      return;
    }
    try {
      await fs.promises.mkdir(dir, { recursive: true });
      const target = path.join(dir, name);
      // Read binary body
      const chunks: Buffer[] = [];
      await new Promise<void>((resolve, reject) => {
        req.on('data', (chunk) => chunks.push(Buffer.from(chunk)));
        req.on('end', resolve);
        req.on('error', reject);
      });
      const body = Buffer.concat(chunks);
      await fs.promises.writeFile(target, body);
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ ok: true, path: target, size: body.length }));
    } catch (err: any) {
      res.writeHead(500, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: err?.message || 'Write failed' }));
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
