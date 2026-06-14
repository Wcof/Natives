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
 * 判断是否应该忽略此文件变更事件
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
 * @param dirPath 监控目录
 * @param cb 回调函数
 * @returns 停止函数
 */
export function startFileWatcher(
  dirPath: string,
  cb: WatchCallback,
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

    const context: WatchContext = {};
    if (!shouldIgnoreEvent(event, context)) {
      cb(event);
    }
  });

  watcher.on('error', () => { /* silently ignore */ });

  return () => {
    try { watcher.close(); } catch { /* ignore */ }
  };
}
