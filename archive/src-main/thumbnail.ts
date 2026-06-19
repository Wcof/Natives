import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import * as crypto from 'crypto';
import { execFile } from 'child_process';
import { execFilePromise } from '@/lib/exec-file';

// ── Constants ──

const IMAGE_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp', '.tiff', '.tif', '.heic', '.avif']);
const VIDEO_EXTENSIONS = new Set(['.mp4', '.mov', '.avi', '.mkv', '.webm', '.m4v']);
const THUMBNAILABLE_EXTENSIONS = new Set([...IMAGE_EXTENSIONS, ...VIDEO_EXTENSIONS, '.pdf']);

let CACHE_DIR = path.join(os.homedir(), '.natives', 'thumbs');
const MAX_CACHE_SIZE = 400 * 1024 * 1024; // 400MB

// ── In-Flight Deduplication（对标 fanbox — 防止并发重复生成）──
const thumbInflight = new Map<string, Promise<{ buffer: Buffer; contentType: string; cached: boolean } | null>>();

// ── Configuration ──

export function setThumbCacheDir(dir: string): void {
  CACHE_DIR = dir;
}

// ── Cache Key ──

function getCacheKey(filePath: string, width: number): string {
  return crypto.createHash('sha256').update(`${filePath}:${width}`).digest('hex');
}

function getCachePath(cacheKey: string): string {
  return path.join(CACHE_DIR, `${cacheKey}.jpg`);
}

// ── Metadata Management ──

interface CacheMeta {
  [cacheKey: string]: { filePath: string; width: number; size: number; lastAccess: number };
}

function getMetaPath(): string {
  return path.join(CACHE_DIR, 'meta.json');
}

async function readMeta(): Promise<CacheMeta> {
  try {
    const raw = await fs.promises.readFile(getMetaPath(), 'utf-8');
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

async function writeMeta(meta: CacheMeta): Promise<void> {
  await fs.promises.mkdir(CACHE_DIR, { recursive: true });
  await fs.promises.writeFile(getMetaPath(), JSON.stringify(meta), 'utf-8');
}

async function updateAccess(cacheKey: string): Promise<void> {
  const meta = await readMeta();
  if (meta[cacheKey]) {
    meta[cacheKey]!.lastAccess = Date.now();
    await writeMeta(meta);
  }
}

// ── LRU Eviction ──

async function enforceCacheLimit(): Promise<void> {
  const meta = await readMeta();
  const keys = Object.keys(meta);
  if (keys.length === 0) return;

  // 计算总大小
  let totalSize = 0;
  for (const key of keys) {
    totalSize += meta[key]!.size || 0;
  }

  if (totalSize <= MAX_CACHE_SIZE) return;

  // LRU 淘汰：按 lastAccess 升序排序
  const sorted = keys.sort((a, b) => (meta[a]!.lastAccess || 0) - (meta[b]!.lastAccess || 0));

  for (const key of sorted) {
    if (totalSize <= MAX_CACHE_SIZE) break;
    const entry = meta[key]!;
    try {
      await fs.promises.unlink(getCachePath(key));
    } catch { /* 文件可能已被删除 */ }
    totalSize -= entry.size || 0;
    delete meta[key];
  }

  await writeMeta(meta);
}

// ── Generate Thumbnail (with cache) ──

/**
 * 生成缩略图（带缓存）
 * @param filePath 文件路径
 * @param width 目标宽度（48-1600px）
 * @returns 缩略图 Buffer + Content-Type，不支持的类型返回 null
 */
export async function generateThumb(
  filePath: string,
  width: number,
): Promise<{ buffer: Buffer; contentType: string; cached: boolean } | null> {
  const thumbWidth = Math.max(48, Math.min(1600, width));

  // 检查文件存在性
  try {
    const stat = await fs.promises.stat(filePath);
    if (!stat.isFile()) return null;
  } catch {
    return null;
  }

  const ext = path.extname(filePath).toLowerCase();
  if (!THUMBNAILABLE_EXTENSIONS.has(ext)) return null;

  const cacheKey = getCacheKey(filePath, thumbWidth);
  const cachePath = getCachePath(cacheKey);

  // 检查缓存
  try {
    const buffer = await fs.promises.readFile(cachePath);
    await updateAccess(cacheKey);
    return { buffer, contentType: 'image/jpeg', cached: true };
  } catch {
    // 缓存未命中，继续生成
  }

  // ── In-Flight Dedup：如果同一缩略图正在生成中，复用其 Promise ──
  const inflight = thumbInflight.get(cacheKey);
  if (inflight) return inflight;

  const promise = (async () => {
    // 确保缓存目录存在
    await fs.promises.mkdir(CACHE_DIR, { recursive: true });

    const tmpDir = path.join(os.tmpdir(), 'natives-thumbs');
    await fs.promises.mkdir(tmpDir, { recursive: true });

    const tmpPath = path.join(tmpDir, `thumb-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.jpg`);

    try {
      if (IMAGE_EXTENSIONS.has(ext)) {
        await execFilePromise('sips', ['-Z', String(thumbWidth), '-s', 'format', 'jpeg', filePath, '--out', tmpPath]);
      } else {
        await execFilePromise('qlmanage', ['-t', '-s', String(thumbWidth), '-o', tmpDir, filePath]);
        const basename = path.basename(filePath, ext);
        const qlOutput = path.join(tmpDir, `${basename}.png`);
        if (fs.existsSync(qlOutput)) {
          await execFilePromise('sips', ['-s', 'format', 'jpeg', qlOutput, '--out', tmpPath]);
          try { await fs.promises.unlink(qlOutput); } catch { /* ignore */ }
        }
      }

      const buffer = await fs.promises.readFile(tmpPath);

      // 写入缓存
      try {
        await fs.promises.copyFile(tmpPath, cachePath);
        const meta = await readMeta();
        meta[cacheKey] = {
          filePath,
          width: thumbWidth,
          size: buffer.length,
          lastAccess: Date.now(),
        };
        await writeMeta(meta);
        await enforceCacheLimit();
      } catch { /* 缓存写入失败不影响功能 */ }

      try { await fs.promises.unlink(tmpPath); } catch { /* ignore */ }

      return { buffer, contentType: 'image/jpeg', cached: false };
    } catch {
      try { if (fs.existsSync(tmpPath)) await fs.promises.unlink(tmpPath); } catch { /* ignore */ }
      return null;
    }
  })();

  thumbInflight.set(cacheKey, promise);
  try {
    return await promise;
  } finally {
    thumbInflight.delete(cacheKey);
  }
}


