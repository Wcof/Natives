/**
 * scripts/perf/generate-fixtures.ts
 *
 * T02 perf baseline — deterministic fixture generator for the file-browser benchmark.
 *
 * Generates, entirely in memory (seeded PRNG, reproducible across runs):
 *   - simulated directory metadata arrays (DirEntry { name, kind, size, mtime })
 *     for 1k / 5k / 10k / 50k entries
 *   - a 500-image listing (ImageEntry { name, width, height, size, mtime })
 *   - deep watch `modify` event streams (1k / 5k events) spread across a 1 s
 *     window, on nested paths such as `projects/acme/frontend/src/components/Button.tsx`
 *
 * Run directly to ALSO materialise the fixtures as JSON for offline inspection:
 *   npx tsx scripts/perf/generate-fixtures.ts [outDir]
 *   # default outDir: scripts/perf/fixtures
 *
 * The benchmark harness (scripts/perf/file-browser-perf.ts) imports the exported
 * functions and does not depend on the on-disk copies, so it runs standalone:
 *   npx tsx scripts/perf/file-browser-perf.ts
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const DEFAULT_SEED = 20260808;

export const DIR_SIZES = [1000, 5000, 10000, 50000] as const;
export const IMAGE_COUNT = 500;
export const WATCH_SIZES = [1000, 5000] as const;
export const WATCH_WINDOW_MS = 1000;
export const WATCH_DEPTH = 5;

export type DirKind = 'file' | 'dir';

export interface DirEntry {
  name: string;
  kind: DirKind;
  size: number; // bytes (0 for directories)
  mtime: number; // epoch ms
}

export interface ImageEntry {
  name: string;
  width: number;
  height: number;
  size: number;
  mtime: number;
}

export interface WatchEvent {
  ts: number; // ms offset within the stream window [0, WATCH_WINDOW_MS)
  kind: 'modify';
  path: string; // deep nested path, e.g. projects/acme/frontend/src/Button.tsx
}

/** Deterministic 32-bit PRNG (mulberry32). */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randInt(rng: () => number, min: number, max: number): number {
  return min + Math.floor(rng() * (max - min + 1));
}

function pick<T>(rng: () => number, arr: readonly T[]): T {
  const item = arr[Math.floor(rng() * arr.length)];
  if (item === undefined) throw new Error('pick: empty array');
  return item;
}

const DIR_PREFIXES = ['src', 'docs', 'assets', 'projects', 'components', 'lib', 'api', 'config', 'tests', 'public'];
const FILE_EXTENSIONS = ['ts', 'tsx', 'js', 'json', 'md', 'css', 'html', 'svg', 'png', 'yaml'];

const IMAGE_CAMERAS = ['DSC', 'IMG', 'PXL', 'Screenshot', 'Pano', 'HDR', 'Live'];
const IMAGE_EXTENSIONS = ['jpg', 'jpeg', 'png', 'webp', 'heic'];

const WATCH_LAYERS = [
  'projects',
  'acme',
  'frontend',
  'src',
  'components',
  'hooks',
  'ui',
  'pages',
  'api',
  'lib',
  'utils',
  'tests',
  'node_modules',
  '.github',
  'config',
];
const WATCH_EXTENSIONS = ['ts', 'tsx', 'js', 'json', 'css', 'md', 'svg', 'yaml'];

const NOW = Date.now();
const DAY_MS = 24 * 60 * 60 * 1000;

export function generateDirEntries(count: number, seed: number = DEFAULT_SEED): DirEntry[] {
  const rng = mulberry32(seed ^ 0x1f2e3d4c);
  const entries: DirEntry[] = [];
  for (let i = 0; i < count; i++) {
    const isDir = rng() < 0.2;
    const prefix = pick(rng, DIR_PREFIXES);
    const name = isDir
      ? `${prefix}-${String(i).padStart(4, '0')}`
      : `${prefix}-${String(i).padStart(4, '0')}.${pick(rng, FILE_EXTENSIONS)}`;
    entries.push({
      name,
      kind: isDir ? 'dir' : 'file',
      size: isDir ? 0 : randInt(rng, 0, 2 * 1024 * 1024),
      mtime: NOW - randInt(rng, 0, 30 * DAY_MS),
    });
  }
  return entries;
}

export function generateImages(count: number = IMAGE_COUNT, seed: number = DEFAULT_SEED): ImageEntry[] {
  const rng = mulberry32(seed ^ 0x5a1e2b3c);
  const images: ImageEntry[] = [];
  for (let i = 0; i < count; i++) {
    const width = pick(rng, [640, 800, 1024, 1280, 1440, 1920, 2560, 3840] as const);
    const height = pick(rng, [360, 480, 720, 800, 900, 1080, 1440, 2160] as const);
    images.push({
      name: `${pick(rng, IMAGE_CAMERAS)}_${String(i + 1).padStart(4, '0')}.${pick(rng, IMAGE_EXTENSIONS)}`,
      width,
      height,
      size: Math.round(width * height * (0.08 + rng() * 0.4)),
      mtime: NOW - randInt(rng, 0, 365 * DAY_MS),
    });
  }
  return images;
}

export function generateWatchModifyEvents(
  count: number,
  seed: number = DEFAULT_SEED,
  depth: number = WATCH_DEPTH,
  windowMs: number = WATCH_WINDOW_MS,
): WatchEvent[] {
  const rng = mulberry32(seed ^ 0x77e10f31);
  const events: WatchEvent[] = [];
  for (let i = 0; i < count; i++) {
    const segments: string[] = [];
    for (let d = 0; d < depth; d++) {
      segments.push(pick(rng, WATCH_LAYERS));
    }
    const fileName = `f-${String(i).padStart(5, '0')}.${pick(rng, WATCH_EXTENSIONS)}`;
    events.push({
      ts: rng() * windowMs,
      kind: 'modify',
      path: [...segments, fileName].join('/'),
    });
  }
  events.sort((a, b) => a.ts - b.ts);
  return events;
}

export interface FixtureSet {
  dirEntries: Record<number, DirEntry[]>;
  images: ImageEntry[];
  watchModify: Record<number, WatchEvent[]>;
}

export function buildFixtureSet(seed: number = DEFAULT_SEED): FixtureSet {
  const dirEntries: Record<number, DirEntry[]> = {};
  for (const size of DIR_SIZES) {
    dirEntries[size] = generateDirEntries(size, seed);
  }
  const images = generateImages(IMAGE_COUNT, seed);
  const watchModify: Record<number, WatchEvent[]> = {};
  for (const size of WATCH_SIZES) {
    watchModify[size] = generateWatchModifyEvents(size, seed);
  }
  return { dirEntries, images, watchModify };
}

// ---- CLI: materialise fixtures as JSON (optional) --------------------------

const THIS_FILE = fs.realpathSync(fileURLToPath(import.meta.url));
const ARG_FILE = process.argv[1] ? fs.realpathSync(process.argv[1]) : null;

function defaultOutDir(): string {
  const scriptsPerf = path.dirname(THIS_FILE);
  return path.join(scriptsPerf, 'fixtures');
}

function writeFixtures(outDir: string): void {
  const set = buildFixtureSet();
  fs.mkdirSync(outDir, { recursive: true });
  for (const size of DIR_SIZES) {
    fs.writeFileSync(path.join(outDir, `dir-${size}.json`), JSON.stringify(set.dirEntries[size] ?? [], null, 0));
  }
  fs.writeFileSync(path.join(outDir, `images-${IMAGE_COUNT}.json`), JSON.stringify(set.images, null, 0));
  for (const size of WATCH_SIZES) {
    fs.writeFileSync(path.join(outDir, `watch-modify-${size}.json`), JSON.stringify(set.watchModify[size] ?? [], null, 0));
  }
  const summary = {
    seed: DEFAULT_SEED,
    dirSizes: [...DIR_SIZES],
    imageCount: IMAGE_COUNT,
    watchSizes: [...WATCH_SIZES],
    watchWindowMs: WATCH_WINDOW_MS,
    watchDepth: WATCH_DEPTH,
    generatedAt: new Date().toISOString(),
  };
  fs.writeFileSync(path.join(outDir, 'summary.json'), JSON.stringify(summary, null, 2));
  console.log(`[generate-fixtures] wrote fixtures to ${outDir}`);
  console.log(
    `[generate-fixtures] dir entries ${DIR_SIZES.join('/')}, images ${IMAGE_COUNT}, watch modify ${WATCH_SIZES.join('/')}`,
  );
}

if (ARG_FILE === THIS_FILE) {
  const outDir = process.argv[2] ? path.resolve(process.argv[2]) : defaultOutDir();
  writeFixtures(outDir);
}
