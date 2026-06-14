import * as fs from 'fs';
import * as path from 'path';
import { Readable } from 'stream';
import { type FileEntry, detectFileKind, detectProjectBadge } from '../types/file';

// ── List Directory ──

interface ListDirOptions {
  sortBy?: 'name' | 'mtime' | 'size';
  sortDir?: 'asc' | 'desc';
  showHidden?: boolean;
}

const DS_STORE = '.DS_Store';

/**
 * 列出目录内容
 * @param dirPath 目录路径
 * @param options 排序/过滤选项
 * @returns 文件条目列表
 */
export async function listDir(dirPath: string, options?: ListDirOptions): Promise<FileEntry[]> {
  const stat = await fs.promises.stat(dirPath);
  if (!stat.isDirectory()) {
    throw Object.assign(new Error(`Not a directory: ${dirPath}`), { code: 'ENOTDIR' });
  }

  const entries = await fs.promises.readdir(dirPath);

  // 过滤 .DS_Store
  const filtered = entries.filter((name) => name !== DS_STORE);

  const result: FileEntry[] = [];

  for (const name of filtered) {
    // 隐藏文件过滤（.DS_Store 已过滤，其余隐藏文件根据 showHidden 决定）
    const isHidden = name.startsWith('.');
    if (isHidden && !options?.showHidden) continue;

    const fullPath = path.resolve(dirPath, name);

    try {
      // 先 lstat 获取链接本身的信息
      const lstat = await fs.promises.lstat(fullPath);
      const isSymlink = lstat.isSymbolicLink();

      let symlinkTarget: string | undefined;
      let targetStat: fs.Stats;

      if (isSymlink) {
        symlinkTarget = await fs.promises.readlink(fullPath);
        // 尝试 stat（解析符号链接），如果目标不存在则 fallback 到 lstat
        try {
          targetStat = await fs.promises.stat(fullPath);
        } catch {
          targetStat = lstat;
        }
      } else {
        targetStat = lstat;
      }

      const isDir = targetStat.isDirectory();
      const kind = isDir ? 'other' : detectFileKind(name);

      // 检测项目徽章（仅在目录中检测）
      let projectBadge: string | undefined;
      if (isDir) {
        try {
          const childFiles = await fs.promises.readdir(fullPath);
          const hasGit = childFiles.includes('.git');
          projectBadge = detectProjectBadge(fullPath, childFiles, hasGit);
        } catch {
          // 无权限读取的目录
        }
      }

      result.push({
        name,
        path: fullPath,
        isDir,
        kind,
        hidden: isHidden,
        size: isDir ? 4096 : targetStat.size,
        mtime: targetStat.mtimeMs,
        btime: targetStat.birthtimeMs,
        ...(symlinkTarget ? { symlink: symlinkTarget } : {}),
        ...(projectBadge ? { projectBadge: projectBadge as any } : {}),
      });
    } catch {
      // 无法 stat 的文件（权限问题等），跳过
      continue;
    }
  }

  // ── 排序 ──
  const sortBy = options?.sortBy || 'name';
  const sortDir = options?.sortDir || 'asc';

  result.sort((a, b) => {
    let cmp: number;

    switch (sortBy) {
      case 'mtime':
        cmp = a.mtime - b.mtime;
        break;
      case 'size':
        cmp = a.size - b.size;
        break;
      case 'name':
      default:
        // 中文排序
        cmp = a.name.localeCompare(b.name, 'zh-CN', { numeric: true });
        break;
    }

    return sortDir === 'desc' ? -cmp : cmp;
  });

  return result;
}

// ── Truncation Constants ──

const MAX_FULL_READ = 2 * 1024 * 1024; // 2MB
const MAX_TRUNCATED_READ = 256 * 1024;  // 256KB

/**
 * 读取文件内容
 * @param filePath 文件路径
 * @returns 文件内容 + 元数据
 */
export async function readFile(filePath: string): Promise<{
  content: string;
  truncated: boolean;
  size: number;
  encoding: string;
}> {
  const stat = await fs.promises.stat(filePath);

  if (!stat.isFile()) {
    throw Object.assign(new Error(`Not a file: ${filePath}`), { code: 'EISDIR' });
  }

  const fileSize = stat.size;
  const encoding = 'utf-8';
  const truncated = fileSize > MAX_FULL_READ;

  let content: string;
  if (truncated) {
    const buffer = Buffer.alloc(Math.min(MAX_TRUNCATED_READ, fileSize));
    const fd = await fs.promises.open(filePath, 'r');
    try {
      await fd.read(buffer, 0, buffer.length, 0);
    } finally {
      await fd.close();
    }
    content = buffer.toString(encoding).replace(/\0+$/, '');
  } else {
    content = await fs.promises.readFile(filePath, encoding);
  }

  return { content, truncated, size: fileSize, encoding };
}

// ── MIME Types ──

const MIME_TYPES: Record<string, string> = {
  '.txt': 'text/plain',
  '.md': 'text/markdown',
  '.html': 'text/html',
  '.htm': 'text/html',
  '.css': 'text/css',
  '.js': 'application/javascript',
  '.mjs': 'application/javascript',
  '.cjs': 'application/javascript',
  '.json': 'application/json',
  '.xml': 'application/xml',
  '.yaml': 'text/yaml',
  '.yml': 'text/yaml',
  '.toml': 'text/toml',
  '.ts': 'application/typescript',
  '.tsx': 'application/typescript',
  '.jsx': 'application/javascript',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.bmp': 'image/bmp',
  '.pdf': 'application/pdf',
  '.zip': 'application/zip',
  '.tar': 'application/x-tar',
  '.gz': 'application/gzip',
  '.mp3': 'audio/mpeg',
  '.wav': 'audio/wav',
  '.ogg': 'audio/ogg',
  '.mp4': 'video/mp4',
  '.webm': 'video/webm',
  '.mov': 'video/quicktime',
};

function getMimeType(filePath: string): string {
  const lower = filePath.toLowerCase();
  for (const [ext, mime] of Object.entries(MIME_TYPES)) {
    if (lower.endsWith(ext)) return mime;
  }
  return 'application/octet-stream';
}

// ── Stream File ──

interface StreamFileRange {
  start: number;
  end?: number;
}

interface StreamFileResult {
  stream: Readable;
  totalSize: number;
  contentRange?: string;
  contentType: string;
}

/**
 * 流式读取文件（支持 Range 请求）
 * @param filePath 文件路径
 * @param range 可选的字节范围
 */
export async function streamFile(
  filePath: string,
  range?: StreamFileRange,
): Promise<StreamFileResult> {
  const stat = await fs.promises.stat(filePath);
  if (!stat.isFile()) {
    throw Object.assign(new Error(`Not a file: ${filePath}`), { code: 'EISDIR' });
  }

  const totalSize = stat.size;
  const contentType = getMimeType(filePath);

  if (range) {
    const start = Math.max(0, range.start);
    const end = range.end !== undefined ? Math.min(range.end, totalSize - 1) : totalSize - 1;
    const chunkSize = end - start + 1;

    const stream = fs.createReadStream(filePath, { start, end });
    const contentRange = `bytes ${start}-${end}/${totalSize}`;

    return { stream, totalSize, contentRange, contentType };
  }

  return {
    stream: fs.createReadStream(filePath),
    totalSize,
    contentType,
  };
}
