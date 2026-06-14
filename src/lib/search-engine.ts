import * as fs from 'fs';
import * as path from 'path';
import { type ContentSearchResult, type SearchResult } from '../types/file';

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
    // 前一个字符是分隔符
    const prev = filename[pos - 1]!;
    if (prev === '-' || prev === '_' || prev === '.' || prev === ' ' || prev === '/') {
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
  if (mtime) {
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

// ── grepContent ──

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
  const results: ContentSearchResult[] = [];
  const maxResults = options?.maxResults || 50;
  const contextLines = options?.contextLines || 0;
  const allowedExts = options?.fileExtensions
    ? new Set(options.fileExtensions.map((e) => e.startsWith('.') ? e : `.${e}`))
    : null;
  const q = query.toLowerCase();

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

    try {
      const entries = await fs.promises.readdir(dirPath, { withFileTypes: true });
      for (const entry of entries) {
        if (results.length >= maxResults) return;
        const fullPath = path.join(dirPath, entry.name);
        if (entry.isDirectory()) {
          // 跳过 node_modules, .git 等
          if (entry.name.startsWith('.')) continue;
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

  async function walk(dirPath: string): Promise<void> {
    if (results.length >= maxResults * 2) return; // 收集更多以便排序

    try {
      const entries = await fs.promises.readdir(dirPath, { withFileTypes: true });
      for (const entry of entries) {
        if (results.length >= maxResults * 2) break;
        const fullPath = path.join(dirPath, entry.name);

        if (entry.isDirectory()) {
          // 跳过隐藏目录
          if (entry.name.startsWith('.')) continue;
          if (includeDirs) {
            const score = calculateScore(query, entry.name, true);
            if (score !== -1) {
              results.push({
                path: fullPath,
                name: entry.name,
                score,
                isDir: true,
                mtime: 0,
                matchRanges: [],
              });
            }
          }
          await walk(fullPath);
        } else if (entry.isFile()) {
          const score = calculateScore(query, entry.name);
          if (score !== -1) {
            results.push({
              path: fullPath,
              name: entry.name,
              score,
              isDir: false,
              mtime: 0,
              matchRanges: [],
            });
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
