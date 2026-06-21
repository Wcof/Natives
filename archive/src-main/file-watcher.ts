import * as fs from 'fs';
import * as path from 'path';
import { type FileChangeEvent } from '../types/agent';

// ── Export types for testing ──

export type { FileChangeEvent };

export interface WatchContext {
  activeFilePath?: string;
  lastUserAccessTime?: number;
}

// ── Noise Filtering ──

const SIDECAR_PATTERNS = ['-journal', '-shm', '-wal', '.tmp', '.lock'];

/**
 * 判断是否应该忽略此文件变更事件（基于文件名模式）
 */
export function shouldIgnoreEvent(
  event: FileChangeEvent,
  context: WatchContext,
): boolean {
  const fileName = path.basename(event.path);

  // 1. 忽略隐藏文件
  if (fileName.startsWith('.')) return true;

  // 2. 忽略 SQLite sidecar 文件
  if (SIDECAR_PATTERNS.some((p) => event.path.endsWith(p))) return true;

  // 3. 忽略 node_modules 和 .git
  if (event.path.includes('/node_modules/') || event.path.includes('/.git/')) return true;

  // 4. 3 秒抑制窗口（用户正在浏览的文件）
  if (context.activeFilePath === event.path && context.lastUserAccessTime) {
    if (Date.now() - context.lastUserAccessTime < 3000) return true;
  }

  return false;
}

// ── Stat-Based Cache（对标 Natives2 的 mtime+size 缓存） ──

/** 文件元数据缓存 — 跳过 metadata-only 的 FSEvents */
const statCache = new Map<string, { size: number; mtimeMs: number }>();

/** Per-file 防抖 — 合并快速连续事件 */
const debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();
const DEBOUNCE_MS = 300;

/**
 * 检查文件内容是否真正变更（stat-based，对标 Natives2）
 * 返回 true 表示文件未变更（应跳过）
 */
function isMetadataOnly(filePath: string): boolean {
  try {
    const st = fs.statSync(filePath);
    const cached = statCache.get(filePath);
    if (cached && cached.size === st.size && cached.mtimeMs === st.mtimeMs) {
      return true; // size 和 mtime 都没变 → metadata-only 事件
    }
    statCache.set(filePath, { size: st.size, mtimeMs: st.mtimeMs });
    return false;
  } catch {
    // 文件不存在（delete 事件）— 不是 metadata-only
    statCache.delete(filePath);
    return false;
  }
}

/**
 * 清理 stat 缓存中已删除的文件（定期调用）
 */
export function evictDeletedFromStatCache(): void {
  for (const filePath of statCache.keys()) {
    try { fs.statSync(filePath); } catch { statCache.delete(filePath); }
  }
}

// ── Priority Queue ──

const HIGH_PRIORITY_EXTS = new Set(['.html', '.htm', '.md', '.mdx']);
const MEDIUM_PRIORITY_EXTS = new Set(['.ts', '.tsx', '.js', '.jsx', '.py', '.rs', '.go']);

/**
 * 获取文件优先级 (1=最高, 3=最低)
 */
export function getFilePriority(filePath: string): number {
  const ext = path.extname(filePath).toLowerCase();

  if (HIGH_PRIORITY_EXTS.has(ext)) return 1;
  if (MEDIUM_PRIORITY_EXTS.has(ext)) return 2;
  return 3;
}

// ── File Watcher ──

type WatchCallback = (event: FileChangeEvent) => void;

/**
 * 启动文件监控
 *
 * 噪声过滤策略（对标 Natives2 + CodePilot）：
 * 1. 文件名模式过滤（隐藏文件、sidecar、node_modules）
 * 2. Stat-based 内容变更检测（mtime+size，跳过 metadata-only FSEvents）
 * 3. Per-file 防抖（300ms，合并快速连续事件）
 * 4. 活跃文件 3 秒抑制窗口
 *
 * @param dirPath 监控目录
 * @param cb 回调函数
 * @param context 可选的 WatchContext（活跃文件路径、用户访问时间）
 * @returns 停止函数
 */
export function startFileWatcher(
  dirPath: string,
  cb: WatchCallback,
  context?: WatchContext,
): () => void {
  const watcher = fs.watch(dirPath, { recursive: true }, (eventType, filename) => {
    if (!filename) return;

    const fullPath = path.resolve(dirPath, filename.toString());
    const event: FileChangeEvent = {
      path: fullPath,
      type: eventType === 'rename' ? 'delete' : 'modify',
      timestamp: Date.now(),
    };

    // 如果文件是新增的，尝试区分 create vs modify
    if (eventType === 'rename') {
      try {
        fs.accessSync(fullPath);
        event.type = 'create';
      } catch {
        event.type = 'delete';
      }
    }

    // 第一层：文件名模式过滤
    if (shouldIgnoreEvent(event, context ?? {})) return;

    // 第二层：stat-based 内容变更检测（跳过 metadata-only 事件）
    if (event.type !== 'delete' && isMetadataOnly(fullPath)) return;

    // 第三层：per-file 防抖（合并 300ms 内的连续事件）
    const existing = debounceTimers.get(fullPath);
    if (existing) clearTimeout(existing);
    debounceTimers.set(fullPath, setTimeout(() => {
      debounceTimers.delete(fullPath);
      cb(event);
    }, DEBOUNCE_MS));
  });

  watcher.on('error', () => { /* silently ignore */ });

  return () => {
    try {
      watcher.close();
    } catch { /* ignore */ }
    // 清理防抖定时器
    for (const timer of debounceTimers.values()) clearTimeout(timer);
    debounceTimers.clear();
  };
}
