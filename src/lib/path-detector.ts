import { resolve as pathResolve, isAbsolute as pathIsAbsolute, dirname as pathDirname, basename as pathBasename, extname as pathExtname, join as pathJoin } from '@/lib/path-utils';
import { fsApi, hasNativeFiles, searchApi } from '@/lib/files-api';

/* ── Types ── */

export interface PathMatch {
  path: string;
  line?: number;
  column?: number;
  start: number;
  end: number;
  /** true = verified to exist on disk; false = not checked; 'truncated' = has … and needs basename search */
  verified?: boolean | 'truncated';
}

interface ScrollbackEntry {
  text: string;
  timestamp: number;
}

/* ── Constants ── */

const SCROLLBACK_MAX_LINES = 200;
const SCROLLBACK_MAX_AGE_MS = 30_000; // 30 seconds

/* ── Scrollback Buffer ── */

const scrollbackBuffer: ScrollbackEntry[] = [];

/** Record a terminal output line for scrollback path scanning. */
export function recordScrollbackLine(line: string) {
  if (!line.trim()) return;
  scrollbackBuffer.push({ text: line.trim(), timestamp: Date.now() });
  while (scrollbackBuffer.length > SCROLLBACK_MAX_LINES) scrollbackBuffer.shift();
  const cutoff = Date.now() - SCROLLBACK_MAX_AGE_MS;
  while (scrollbackBuffer.length > 0 && scrollbackBuffer[0]!.timestamp < cutoff) scrollbackBuffer.shift();
}

export function clearScrollbackBuffer() {
  scrollbackBuffer.length = 0;
}

/* ── OSC 7 PWD tracking（overrides scrollback-derived cwd） ── */

/** Per-session authoritative cwd (set by OSC 7 / Ghostty pwd event) */
const pwdMap = new Map<string, string>();

/** Record an authoritative cwd from OSC 7 sequence (Ghostty pwd-changed event). */
export function recordPwdChange(sessionId: string, pwd: string) {
  if (!pwd) return;
  pwdMap.set(sessionId, pwd);
}

/** Get the last known authoritative cwd for a session (or undefined). */
export function getSessionPwd(sessionId: string): string | undefined {
  return pwdMap.get(sessionId);
}

/** Get scrollback lines within the time window (for FollowRenderer action parsing). */
export function getScrollbackLines(): string[] {
  const cutoff = Date.now() - SCROLLBACK_MAX_AGE_MS;
  return scrollbackBuffer
    .filter(entry => entry.timestamp >= cutoff)
    .map(entry => entry.text);
}

/* ── Core Detection ── */

/**
 * Detect file paths in text using three regex passes with priority ordering.
 * Does NOT verify existence — use locatePath() on click for that.
 */
export function detectFilePaths(text: string, currentDir: string): PathMatch[] {
  const matches: PathMatch[] = [];
  let match: RegExpExecArray | null;

  // 1. Line references: file.ts:42 or file.ts:42:10 (highest priority)
  const linePattern = /([\w\-.]+\.\w+):(\d+)(?::(\d+))?/g;
  while ((match = linePattern.exec(text)) !== null) {
    const resolved = pathResolve(currentDir, match[1]!);
    matches.push({
      path: resolved,
      line: parseInt(match[2]!, 10),
      column: match[3] ? parseInt(match[3]!, 10) : undefined,
      start: match.index,
      end: match.index + match[0].length,
    });
  }

  // 2. Truncated paths with … (Natives2: give underline immediately, verify on click)
  //    e.g., src/…/file.ts, ~/…/components/App.tsx
  const truncPattern = /([\w\-.~/]+)?[.…]+([\w\-.]+\.\w+)/g;
  while ((match = truncPattern.exec(text)) !== null) {
    if (!isOverlapping(matches, match.index, match.index + match[0].length)) {
      matches.push({
        path: match[0],
        start: match.index,
        end: match.index + match[0].length,
        verified: 'truncated',
      });
    }
  }

  // 3. Relative paths: ./src/file.ts or ../file.ts
  const relPattern = /(\.\.?\/[\w\-.]+(?:\/[\w\-.]+)*\.\w+)/g;
  while ((match = relPattern.exec(text)) !== null) {
    const resolved = pathResolve(currentDir, match[1]!);
    if (!isOverlapping(matches, match.index, match.index + match[0].length)) {
      matches.push({ path: resolved, start: match.index, end: match.index + match[0].length });
    }
  }

  // 4. Absolute paths: /Users/xxx/file.ts
  const absPattern = /(\/[^\s:"'<>&|;`!@#$%^*()+={}[\]?]+\.\w+)/g;
  while ((match = absPattern.exec(text)) !== null) {
    if (!isOverlapping(matches, match.index, match.index + match[0].length)) {
      matches.push({ path: match[1]!, start: match.index, end: match.index + match[0].length });
    }
  }

  return matches;
}

/* ── Multi-Strategy Path Resolution (Natives2 locatePath) ── */

/**
 * Try to resolve a path candidate to a real file on disk.
 * Strategy order (Natives2):
 *   1. Direct stat
 *   2. Space expansion (merge adjacent words)
 *   3. Scrollback scan (recent terminal output has more context)
 *   4. Basename search (search cwd + project roots)
 *   5. macOS Spotlight (mdfind) as last resort
 *
 * @param candidate The detected path text (may be truncated or partial)
 * @param currentDir The terminal's current working directory
 * @returns Resolved absolute path, or null if not found
 */
export async function locatePath(
  candidate: string,
  currentDir: string,
): Promise<string | null> {
  // 1. Direct stat
  const direct = await statPath(candidate, currentDir);
  if (direct) return direct;

  // 2. Extension fallback — try appending common extensions
  const expanded = await tryExtensionFallback(candidate, currentDir);
  if (expanded) return expanded;

  // 3. Scrollback scan — look for a more complete version of this path in recent output
  const fromScrollback = await scanScrollbackForPath(candidate, currentDir);
  if (fromScrollback) return fromScrollback;

  // 4+5. Basename search + Spotlight in parallel (first hit wins)
  const fromSearch = await Promise.any([
    basenameSearch(candidate, currentDir),
    spotlightSearch(candidate, currentDir),
  ]).catch(() => null);
  if (fromSearch) return fromSearch;

  return null;
}

/* ── Strategy Implementations ── */

/** Helper: 统一走 files-api 契约取 fs 能力；不可用时返回 undefined（调用方静默降级） */
function getNativeFs(): ReturnType<typeof fsApi> | undefined {
  return hasNativeFiles() ? fsApi() : undefined;
}

/* ── 终端链接接线（W11）：划线前验真 + 点击定位 ── */

/** 验真结果 LRU 缓存（fanbox 600 条），键为绝对/候选路径 */
const verifyCache = new Map<string, boolean>();
const VERIFY_CACHE_MAX = 600;

/**
 * 批量验真候选路径（划线前调用，防中文散文被误标成链接）。
 * 返回存在的路径集合；fs 不可用时返回 null（调用方全部放行，点击兜底）。
 */
export async function verifyCandidates(paths: string[]): Promise<Set<string> | null> {
  const fs = getNativeFs();
  if (!fs?.verifyPaths) return null;
  const hits = new Set<string>();
  const misses: string[] = [];
  for (const p of paths) {
    const cached = verifyCache.get(p);
    if (cached === true) hits.add(p);
    else if (cached === undefined) misses.push(p);
  }
  if (misses.length > 0) {
    try {
      const results = await fs.verifyPaths(misses.slice(0, 32));
      for (const r of results) {
        verifyCache.set(r.path, r.exists);
        if (r.exists) hits.add(r.path);
        if (verifyCache.size > VERIFY_CACHE_MAX) {
          const oldest = verifyCache.keys().next().value;
          if (oldest !== undefined) verifyCache.delete(oldest);
        }
      }
    } catch {
      return null; // 验真通道故障 → 放行
    }
  }
  return hits;
}

/**
 * 点击定位统一入口：后端 fs_locate 四级兜底（直接 stat/空格扩展/多根搜索/
 * spotlight，带时间预算）优先，未命中再走前端 locatePath——
 * scrollback 回扫等只有前端才有的上下文在那边。
 */
export async function locateCandidate(candidate: string, currentDir: string): Promise<string | null> {
  try {
    const fs = getNativeFs();
    if (fs?.locate) {
      const result = await fs.locate(candidate, currentDir);
      if (result?.found && result.path) return result.path;
    }
  } catch { /* fall through */ }
  return locatePath(candidate, currentDir);
}

/** Strategy 1: Direct stat via Tauri IPC (prefers fs.stat, falls back to listDir) */
async function statPath(candidate: string, currentDir: string): Promise<string | null> {
  const resolved = pathIsAbsolute(candidate) ? candidate : pathResolve(currentDir, candidate);
  try {
    const fs = getNativeFs();
    if (!fs) return null;

    if (typeof fs.stat === 'function') {
      const st = await fs.stat(resolved);
      if (st?.found && st.path) return st.path;
      // also try as-is candidate if different
      if (resolved !== candidate) {
        const st2 = await fs.stat(candidate);
        if (st2?.found && st2.path) return st2.path;
      }
      return null;
    }

    const listDir = fs.listDir;
    if (!listDir) return null;
    const parentDir = pathDirname(resolved);
    const basename = pathBasename(resolved);
    const entries = (await listDir(parentDir, { showHidden: true })) as Array<{ name: string; path: string }>;
    const found = entries.find(e => e.name === basename);
    return found?.path ?? null;
  } catch {
    return null;
  }
}

/** Strategy 2: Extension fallback — try appending common extensions to bare filenames */
async function tryExtensionFallback(candidate: string, currentDir: string): Promise<string | null> {
  if (pathExtname(candidate)) return null;

  const commonExts = ['.ts', '.tsx', '.js', '.jsx', '.py', '.rs', '.go', '.json', '.md', '.html', '.css'];
  // Fire all in parallel, return first hit
  const results = await Promise.all(commonExts.map(ext => statPath(candidate + ext, currentDir)));
  return results.find(r => r !== null) ?? null;
}

/** Strategy 3: Scan scrollback buffer for a more complete version of the path */
async function scanScrollbackForPath(candidate: string, currentDir: string): Promise<string | null> {
  const basename = pathBasename(candidate).toLowerCase();
  if (!basename || basename.length < 3) return null;

  // Search scrollback for lines containing the basename
  for (let i = scrollbackBuffer.length - 1; i >= 0; i--) {
    const line = scrollbackBuffer[i]!.text;
    if (!line.toLowerCase().includes(basename)) continue;

    // Re-run detection on this line
    const paths = detectFilePaths(line, currentDir);
    for (const p of paths) {
      if (pathBasename(p.path).toLowerCase() === basename) {
        const result = await statPath(p.path, currentDir);
        if (result) return result;
      }
    }
  }
  return null;
}

/** Strategy 4: Basename search — search cwd + common project roots for files matching basename */
async function basenameSearch(candidate: string, currentDir: string): Promise<string | null> {
  const basename = pathBasename(candidate);
  if (!basename || basename.length < 2) return null;

  try {
    const fs = getNativeFs();
    const listDir = fs?.listDir;
    if (!listDir) return null;

    // Search current directory (non-recursive, just top-level + one level down)
    const roots = [currentDir];
    try {
      const topEntries = await listDir(currentDir, { showHidden: false }) as Array<{ name: string; isDir: boolean }>;
      for (const e of topEntries) {
        if (e.isDir && !e.name.startsWith('.') && e.name !== 'node_modules') {
          roots.push(pathJoin(currentDir, e.name));
        }
      }
    } catch { /* ignore */ }

    // Search all roots in parallel
    const allResults = await Promise.all(roots.map(async root => {
      try {
        const entries = await listDir(root, { showHidden: false }) as Array<{ name: string; path: string }>;
        return entries.find(e => e.name === basename)?.path ?? null;
      } catch { return null; }
    }));
    return allResults.find(r => r !== null) ?? null;
  } catch { /* ignore */ }
  return null;
}

/** Strategy 5: macOS Spotlight (mdfind) as last resort */
async function spotlightSearch(candidate: string, currentDir: string): Promise<string | null> {
  const basename = pathBasename(candidate);
  if (!basename || basename.length < 3) return null;

  try {
    // files-api 契约：search 不可用时抛错走 catch 静默降级（与原可选链语义等价）
    const query = `kMDItemDisplayName == "*${basename}*"`;
    // Use kMDItemDisplayName for CJK-safe substring matching (Natives2 approach)
    const results = (await searchApi().spotlight(query, currentDir)) as Array<{ path: string }>;
    if (results.length > 0) return results[0]!.path;
  } catch { /* ignore */ }
  return null;
}

/* ── Helpers ── */

function isOverlapping(matches: PathMatch[], start: number, end: number): boolean {
  return matches.some((m) => m.start <= end && m.end >= start);
}
