import * as fs from 'fs';
import * as path from 'path';
import { execFile, execFileSync } from 'child_process';
import { type ContentSearchResult, type SearchResult } from '../types/file';
import { execFilePromise } from '@/lib/exec-file';

/**
 * 子序列匹配
 * @param query 搜索词
 * @param target 目标文件名
 * @returns 匹配位置数组，不匹配返回 null
 */
export function findSubsequence(query: string, target: string): number[] | null {
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  const positions: number[] = [];
  let qi = 0;

  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      positions.push(ti);
      qi++;
    }
  }

  return qi === q.length ? positions : null;
}

/**
 * 连续匹配（Streak）计算
 * @param positions 匹配位置数组
 * @returns 连续段长度数组
 */
export function findStreaks(positions: number[]): number[] {
  const streaks: number[] = [];
  let currentStreak = 1;

  for (let i = 1; i < positions.length; i++) {
    if (positions[i]! === positions[i - 1]! + 1) {
      currentStreak++;
    } else {
      streaks.push(currentStreak);
      currentStreak = 1;
    }
  }
  streaks.push(currentStreak);

  return streaks;
}

/**
 * 计算词边界匹配数
 */
function countWordBoundaryMatches(positions: number[], filename: string): number {
  let count = 0;
  for (const pos of positions) {
    // 位置 0 就是词边界
    if (pos === 0) {
      count++;
      continue;
    }
    const prev = filename[pos - 1]!;
    const curr = filename[pos]!;
    // 分隔符边界（- _ . / 空格）
    if (prev === '-' || prev === '_' || prev === '.' || prev === ' ' || prev === '/') {
      count++;
      continue;
    }
    // camelCase 边界：前一个是小写，当前是大写
    if (prev >= 'a' && prev <= 'z' && curr >= 'A' && curr <= 'Z') {
      count++;
    }
  }
  return count;
}

/**
 * 计算文件名模糊搜索评分
 * @param query 搜索词
 * @param filename 文件名
 * @param isDir 是否为目录（可选）
 * @param mtime 修改时间（可选，UNIX ms）
 * @returns 分数，不匹配返回 -1
 */
export function calculateScore(
  query: string,
  filename: string,
  isDir?: boolean,
  mtime?: number,
): number {
  let score = 0;

  // 1. 子序列匹配基础分
  const match = findSubsequence(query, filename);
  if (!match) return -1;

  // 2. 连续匹配奖励（streak bonus）
  const streaks = findStreaks(match);
  score += streaks.reduce((sum, s) => sum + s * s, 0) * 10;

  // 3. 词边界奖励（word boundary bonus）
  const wordBoundaries = countWordBoundaryMatches(match, filename);
  score += wordBoundaries * 15;

  // 4. 位置奖励（earlier = better）
  const firstMatchPos = match[0]!;
  score += Math.max(0, 10 - firstMatchPos);

  // 5. 长度惩罚（shorter filename = better）
  score -= filename.length * 0.5;

  // 6. 目录奖励
  if (isDir) score += 5;

  // 7. 最近修改奖励
  if (mtime !== undefined && mtime > 0) {
    const hoursSinceModified = (Date.now() - mtime) / (1000 * 60 * 60);
    score += Math.max(0, 10 - hoursSinceModified * 0.1);
  }

  return score;
}

// ── Binary Detection ──

/** 检查是否二进制文件（内容中包含 null 字节） */
function isBinary(content: Buffer): boolean {
  const len = Math.min(content.length, 8192); // 只检查前 8KB
  for (let i = 0; i < len; i++) {
    if (content[i] === 0) return true;
  }
  return false;
}

const TEXT_EXTENSIONS = new Set([
  '.txt', '.md', '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs',
  '.json', '.yaml', '.yml', '.toml', '.xml', '.css', '.scss',
  '.html', '.htm', '.py', '.rb', '.java', '.rs', '.go', '.c', '.h',
  '.cpp', '.hpp', '.cs', '.swift', '.sh', '.bash', '.zsh', '.env',
  '.conf', '.ini', '.cfg', '.sql', '.graphql', '.vue', '.svelte',
  '.gitignore', '.dockerignore', '.editorconfig',
]);

const MAX_FILE_SIZE = 2 * 1024 * 1024; // 2MB
const SEARCH_DEADLINE_MS = 4_000; // 4 秒硬截止（对标 fanbox，防止大目录无限扫描）

// ── Ripgrep Detection (TASK-011) ──

let _hasRipgrep: boolean | null = null;

function hasRipgrep(): boolean {
  if (_hasRipgrep !== null) return _hasRipgrep;
  try {
    execFileSync('rg', ['--version'], { stdio: 'ignore', timeout: 3000 });
    _hasRipgrep = true;
  } catch {
    _hasRipgrep = false;
  }
  return _hasRipgrep;
}

// ── SQLite FTS5 Index (TASK-011) ──

let _ftsDb: import('better-sqlite3').Database | null = null;

function getFtsDb(): import('better-sqlite3').Database | null {
  if (_ftsDb) return _ftsDb;
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const Database = require('better-sqlite3');
    const dbPath = path.join(process.env.HOME || '/tmp', '.natives', 'search-index.db');
    _ftsDb = new Database(dbPath);
    _ftsDb!.exec(`
      CREATE VIRTUAL TABLE IF NOT EXISTS file_contents USING fts5(
        path UNINDEXED, content, tokenize='unicode61'
      );
    `);
    return _ftsDb;
  } catch {
    return null;
  }
}

/**
 * Index a file's content into the FTS5 search index.
 */
export function indexFileForSearch(filePath: string, content: string): void {
  const db = getFtsDb();
  if (!db) return;
  const normalized = path.resolve(filePath);
  db.prepare('INSERT OR REPLACE INTO file_contents (rowid, path, content) VALUES ((SELECT rowid FROM file_contents WHERE path = ?), ?, ?)')
    .run(normalized, normalized, content);
}

/**
 * Search the FTS5 index.
 */
export function searchFtsIndex(query: string, limit = 50): ContentSearchResult[] {
  const db = getFtsDb();
  if (!db) return [];
  // FTS5 特殊字符清理（防止语法错误）
  const sanitized = query.replace(/["'*+\-()]/g, ' ').trim();
  if (!sanitized) return [];
  try {
    const rows = db.prepare(
      `SELECT path, snippet(file_contents, 1, '<mark>', '</mark>', '...', 40) AS preview
       FROM file_contents WHERE file_contents MATCH ? ORDER BY rank LIMIT ?`
    ).all(sanitized, limit) as Array<{ path: string; preview: string }>;
    return rows.map((r) => ({
      path: r.path,
      name: path.basename(r.path),
      line: 1,
      preview: r.preview,
      matchStart: 0,
      matchEnd: query.length,
    }));
  } catch {
    return [];
  }
}

// ── Ripgrep-based content search (TASK-011) ──

async function grepWithRipgrep(
  query: string,
  root: string,
  options?: { maxResults?: number; fileExtensions?: string[]; contextLines?: number },
): Promise<ContentSearchResult[]> {
  const maxResults = options?.maxResults || 50;
  const args: string[] = [
    '--json',
    '-i',                    // case-insensitive
    '-g', '!.git',
    '-g', '!node_modules',
    '-g', '!.next',
    '-g', '!dist',
    '-g', '!build',
    '--max-count', String(maxResults),
  ];
  if (options?.fileExtensions && options.fileExtensions.length > 0) {
    for (const ext of options.fileExtensions) {
      args.push('-g', `*.${ext}`);
    }
  }
  args.push('--', query, root);

  // 使用异步 execFile 避免阻塞事件循环（对标 fanbox 的异步模式）
  const output = await new Promise<string>((resolve, reject) => {
    execFile('rg', args, { encoding: 'utf-8', timeout: 30000, maxBuffer: 10 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) {
        // rg 退出码 1 = 无匹配，不是错误
        if (err.code === 1 && stdout) {
          resolve(stdout);
        } else {
          reject(err);
        }
      } else {
        resolve(stdout);
      }
    });
  });
  const results: ContentSearchResult[] = [];
  const lines = output.trim().split('\n');
  for (const line of lines) {
    if (!line.trim()) continue;
    try {
      const parsed = JSON.parse(line);
      if (parsed.type === 'match' && parsed.data?.path?.text) {
        results.push({
          path: parsed.data.path.text,
          name: parsed.data.path.text.split('/').pop() || '',
          line: parsed.data.line_number || 1,
          preview: (parsed.data.lines?.text || '').trim(),
          matchStart: 0,
          matchEnd: query.length,
        });
        if (results.length >= maxResults) break;
      }
    } catch { /* skip unparseable lines */ }
  }
  return results;
}

// ── grepContent (TASK-011: ripgrep-first, FTS5 fallback) ──

/**
 * 全文内容搜索
 * @param query 搜索词
 * @param root 搜索根目录
 * @param options 选项
 * @returns 匹配结果
 */
export async function grepContent(
  query: string,
  root: string,
  options?: {
    maxResults?: number;
    fileExtensions?: string[];
    contextLines?: number;
  },
): Promise<ContentSearchResult[]> {
  const maxResults = options?.maxResults || 50;

  // TASK-011: Try ripgrep first (10x-100x faster for large codebases)
  if (hasRipgrep()) {
    try {
      return await grepWithRipgrep(query, root, options);
    } catch {
      // Fall through to Node.js grep
    }
  }

  // Fallback: Node.js line-by-line grep
  const results: ContentSearchResult[] = [];
  const contextLines = options?.contextLines || 0;
  const allowedExts = options?.fileExtensions
    ? new Set(options.fileExtensions.map((e) => e.startsWith('.') ? e : `.${e}`))
    : null;
  const q = query.toLowerCase();
  const deadline = Date.now() + SEARCH_DEADLINE_MS;

  async function searchFile(filePath: string): Promise<void> {
    if (results.length >= maxResults) return;

    // 扩展名过滤
    if (allowedExts) {
      const ext = path.extname(filePath).toLowerCase();
      if (!allowedExts.has(ext)) return;
    }

    try {
      const stat = await fs.promises.stat(filePath);
      if (!stat.isFile() || stat.size > MAX_FILE_SIZE) return;

      const ext = path.extname(filePath).toLowerCase();
      if (!TEXT_EXTENSIONS.has(ext) && ext !== '') return;

      const raw = await fs.promises.readFile(filePath);
      if (isBinary(raw)) return;

      const content = raw.toString('utf-8');
      const lines = content.split('\n');

      for (let i = 0; i < lines.length && results.length < maxResults; i++) {
        const lineLower = lines[i]!.toLowerCase();
        const matchIdx = lineLower.indexOf(q);
        if (matchIdx !== -1) {
          const preview = lines[i]!.substring(
            Math.max(0, matchIdx - 20),
            Math.min(lines[i]!.length, matchIdx + query.length + 20),
          ).trim();
          results.push({
            path: filePath,
            name: path.basename(filePath),
            line: i + 1,
            preview,
            matchStart: matchIdx,
            matchEnd: matchIdx + query.length,
          });
        }
      }
    } catch {
      // 跳过无权限文件
    }
  }

  async function walkDir(dirPath: string): Promise<void> {
    if (results.length >= maxResults) return;
    if (Date.now() >= deadline) return; // 硬截止

    try {
      const entries = await fs.promises.readdir(dirPath, { withFileTypes: true });
      for (const entry of entries) {
        if (results.length >= maxResults) return;
        if (Date.now() >= deadline) return; // 硬截止
        const fullPath = path.join(dirPath, entry.name);
        if (entry.isDirectory()) {
          if (entry.name.startsWith('.')) continue;
          // 跳过常见构建/依赖目录
          if (['node_modules', 'dist', '.next', 'build', 'out', '__pycache__'].includes(entry.name)) continue;
          await walkDir(fullPath);
        } else if (entry.isFile()) {
          await searchFile(fullPath);
        }
      }
    } catch {
      // 跳过无权限目录
    }
  }
  await walkDir(root);
  return results;
}

// ── searchFiles ──

interface SearchFilesOptions {
  maxResults?: number;
  includeDirs?: boolean;
  recencyBonus?: boolean;
}

/**
 * 模糊文件名搜索
 * @param query 搜索词
 * @param root 搜索根目录
 * @param options 选项
 * @returns 排序后的搜索结果
 */
export async function searchFiles(
  query: string,
  root: string,
  options?: SearchFilesOptions,
): Promise<SearchResult[]> {
  const maxResults = options?.maxResults || 80;
  const includeDirs = options?.includeDirs ?? true;
  const results: SearchResult[] = [];
  const deadline = Date.now() + SEARCH_DEADLINE_MS;

  async function walk(dirPath: string): Promise<void> {
    if (results.length >= maxResults * 2) return; // 收集更多以便排序
    if (Date.now() >= deadline) return; // 硬截止

    try {
      const entries = await fs.promises.readdir(dirPath, { withFileTypes: true });
      for (const entry of entries) {
        if (results.length >= maxResults * 2) break;
        if (Date.now() >= deadline) return; // 硬截止
        const fullPath = path.join(dirPath, entry.name);

        if (entry.isDirectory()) {
          // 跳过隐藏目录
          if (entry.name.startsWith('.')) continue;
          if (['node_modules', 'dist', '.next', 'build', 'out', '__pycache__'].includes(entry.name)) continue;
          if (includeDirs) {
            const score = calculateScore(query, entry.name, true);
            if (score !== -1) {
              const matchPos = findSubsequence(query, entry.name);
              const ranges: [number, number][] = matchPos ? matchPos.map((p) => [p, p + 1]) : [];
              try {
                const stat = await fs.promises.stat(fullPath);
                results.push({
                  path: fullPath,
                  name: entry.name,
                  score,
                  isDir: true,
                  mtime: stat.mtimeMs,
                  matchRanges: ranges,
                });
              } catch {
                results.push({
                  path: fullPath,
                  name: entry.name,
                  score,
                  isDir: true,
                  mtime: 0,
                  matchRanges: ranges,
                });
              }
            }
          }
          await walk(fullPath);
        } else if (entry.isFile()) {
          const score = calculateScore(query, entry.name);
          if (score !== -1) {
            const matchPos = findSubsequence(query, entry.name);
            const ranges: [number, number][] = matchPos ? matchPos.map((p) => [p, p + 1]) : [];
            try {
              const stat = await fs.promises.stat(fullPath);
              results.push({
                path: fullPath,
                name: entry.name,
                score,
                isDir: false,
                mtime: stat.mtimeMs,
                matchRanges: ranges,
              });
            } catch {
              results.push({
                path: fullPath,
                name: entry.name,
                score,
                isDir: false,
                mtime: 0,
                matchRanges: ranges,
              });
            }
          }
        }
      }
    } catch {
      // 跳过无权限目录
    }
  }

  await walk(root);

  // 按分数降序排序
  results.sort((a, b) => b.score - a.score);
  return results.slice(0, maxResults);
}

// ── Spotlight Search ──

/**
 * macOS Spotlight 搜索（降级到 grepContent）
 * @param query 搜索词
 * @param root 搜索根目录
 * @returns 匹配结果
 */
export async function spotlightSearch(
  query: string,
  root: string,
): Promise<ContentSearchResult[]> {
  // 优先使用 mdfind (macOS)
  if (process.platform === 'darwin') {
    try {
      const output = await execFilePromise('mdfind', ['-onlyin', root, '-interpret', query]);
      const lines = output.split('\n').filter((l) => l.trim());
      if (lines.length > 0) {
        return lines.slice(0, 50).map((filePath) => ({
          path: filePath,
          name: path.basename(filePath),
          line: 1,
          preview: query,
          matchStart: 0,
          matchEnd: query.length,
        }));
      }
    } catch {
      // mdfind 失败，降级到 grep
    }
  }

  // 降级：使用 grepContent
  return grepContent(query, root);
}


