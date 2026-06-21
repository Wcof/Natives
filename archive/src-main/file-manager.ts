import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { execFile } from 'child_process';
import { Readable } from 'stream';
import { type FileEntry, detectFileKind, detectProjectBadge } from '../types/file';

/**
 * 展开路径中的 ~ 为用户主目录
 */
function expandTilde(p: string): string {
  if (p === '~' || p.startsWith('~/') || p.startsWith('~\\')) {
    return path.join(os.homedir(), p.slice(1));
  }
  return p;
}

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
  dirPath = expandTilde(dirPath);
  const stat = await fs.promises.stat(dirPath);
  if (!stat.isDirectory()) {
    throw Object.assign(new Error(`Not a directory: ${dirPath}`), { code: 'ENOTDIR' });
  }

  const entries = await fs.promises.readdir(dirPath);

  // 过滤 .DS_Store + 隐藏文件
  const filtered = entries.filter((name) => {
    if (name === DS_STORE) return false;
    if (name.startsWith('.') && !options?.showHidden) return false;
    return true;
  });

  // ── 并行 I/O（对标 Natives2 — 批量处理，上限 64 并发）──
  // 顺序 lstat/readdir 对 200 个文件 = 200-600 次串行 I/O。
  // 分批 Promise.all 将 wall-clock 降低到 ~1/64。
  const IO_BATCH = 64;
  const result: FileEntry[] = [];

  async function processEntry(name: string): Promise<FileEntry | null> {
    const fullPath = path.resolve(dirPath, name);
    try {
      const lstat = await fs.promises.lstat(fullPath);
      const isSymlink = lstat.isSymbolicLink();

      let symlinkTarget: string | undefined;
      let targetStat: fs.Stats;

      if (isSymlink) {
        symlinkTarget = await fs.promises.readlink(fullPath);
        try { targetStat = await fs.promises.stat(fullPath); } catch { targetStat = lstat; }
      } else {
        targetStat = lstat;
      }

      const isDir = targetStat.isDirectory();
      const kind = isDir ? 'other' : detectFileKind(name);

      // 检测项目徽章（仅在目录中检测，最多检测 80 个子目录）
      let projectBadge: string | undefined;
      if (isDir && entries.length <= 80) {
        try {
          const childFiles = await fs.promises.readdir(fullPath);
          const hasGit = childFiles.includes('.git');
          projectBadge = detectProjectBadge(fullPath, childFiles, hasGit);
        } catch { /* 无权限 */ }
      }

      return {
        name,
        path: fullPath,
        isDir,
        kind,
        hidden: name.startsWith('.'),
        size: isDir ? 4096 : targetStat.size,
        mtime: targetStat.mtimeMs,
        btime: targetStat.birthtimeMs,
        ...(symlinkTarget ? { symlink: symlinkTarget } : {}),
        ...(projectBadge ? { projectBadge: projectBadge as any } : {}),
      };
    } catch {
      return null;
    }
  }

  // 分批并行处理
  for (let i = 0; i < filtered.length; i += IO_BATCH) {
    const batch = filtered.slice(i, i + IO_BATCH);
    const batchResults = await Promise.all(batch.map(processEntry));
    for (const entry of batchResults) {
      if (entry) result.push(entry);
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
  filePath = expandTilde(filePath);
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
  filePath = expandTilde(filePath);
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

// ── Atomic Write ──

/**
 * 原子写入文件（临时文件 + fsync + rename）
 * @param filePath 目标路径
 * @param content 文件内容
 * @param expectedMtime 可选的期望 mtime（冲突检测）
 * @returns 新的 mtime 和冲突标记
 */
export async function writeFileAtomic(
  filePath: string,
  content: string,
  expectedMtime?: number,
): Promise<{ mtime: number; conflict: boolean }> {
  filePath = expandTilde(filePath);
  const dir = path.dirname(filePath);

  // mtime 冲突检测
  if (expectedMtime !== undefined) {
    try {
      const stat = await fs.promises.stat(filePath);
      const actualMtime = stat.mtimeMs;
      if (Math.abs(actualMtime - expectedMtime) > 1) {
        return { mtime: actualMtime, conflict: true };
      }
    } catch (err: any) {
      // 文件不存在，可以继续
      if (err.code !== 'ENOENT') throw err;
    }
  }

  // 写入临时文件
  const tmpFile = path.join(
    dir,
    `.tmp-${path.basename(filePath)}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
  );

  try {
    const fd = await fs.promises.open(tmpFile, 'wx');
    try {
      await fd.writeFile(content, 'utf-8');
      await fd.sync(); // fsync 确保落盘
    } finally {
      await fd.close();
    }

    // 原子 rename
    await fs.promises.rename(tmpFile, filePath);

    // 获取新文件的 mtime
    const newStat = await fs.promises.stat(filePath);
    return { mtime: newStat.mtimeMs, conflict: false };
  } catch (err) {
    // 清理临时文件
    try {
      await fs.promises.unlink(tmpFile);
    } catch {
      // 临时文件可能不存在
    }
    throw err;
  }
}

// ── Validation ──

/** 允许的根目录（在此范围内可操作文件） */
const ALLOWED_ROOTS: string[] = [
  os.homedir(),
  '/tmp',
  '/private/tmp',
];

/**
 * 路径安全验证（allowlist 模式）
 *
 * 对标 Natives2 的安全模式：
 * - 拒绝空字节
 * - 展开 ~ 后 resolve
 * - 验证解析后的路径在允许的根目录下
 */
function validatePath(targetPath: string): void {
  // 空字节检查
  if (targetPath.includes('\0')) {
    throw Object.assign(new Error(`Path contains null byte: ${targetPath}`), { code: 'EINVAL' });
  }

  // 展开 ~ 并 resolve
  const expanded = expandTilde(targetPath);
  const resolved = path.resolve(expanded);

  // 允许的根目录检查
  const isAllowed = ALLOWED_ROOTS.some((root) => {
    const resolvedRoot = path.resolve(root);
    return resolved === resolvedRoot || resolved.startsWith(resolvedRoot + path.sep);
  });

  if (!isAllowed) {
    throw Object.assign(
      new Error(`Path outside allowed roots: ${targetPath} (resolved: ${resolved})`),
      { code: 'EACCES' },
    );
  }

  // 额外检查：禁止访问敏感的 dotfile 目录（.ssh, .gnupg 等）
  const relativeToHome = path.relative(os.homedir(), resolved);
  const sensitiveDirs = ['.ssh', '.gnupg', '.aws', '.config/gh', '.kube'];
  for (const sensitive of sensitiveDirs) {
    if (relativeToHome === sensitive || relativeToHome.startsWith(sensitive + path.sep)) {
      throw Object.assign(
        new Error(`Access to sensitive directory denied: ${targetPath}`),
        { code: 'EACCES' },
      );
    }
  }
}

/**
 * 创建文件或目录
 * @param targetPath 目标路径
 * @param type 'file' | 'dir'
 */
export async function createEntry(targetPath: string, type: 'file' | 'dir'): Promise<void> {
  validatePath(targetPath);

  if (type === 'dir') {
    await fs.promises.mkdir(targetPath, { recursive: false });
  } else {
    const fd = await fs.promises.open(targetPath, 'wx');
    await fd.close();
  }
}

/**
 * 重命名文件或目录
 * @param oldPath 旧路径
 * @param newPath 新路径
 */
// ── 自动递增文件名辅助函数（消除 3 处重复逻辑）──
/**
 * 如果 targetPath 已存在，自动追加 (1), (2), ... 直到找到可用路径。
 * 最多尝试 100 次。
 */
async function deduplicatePath(targetPath: string): Promise<string> {
  try {
    await fs.promises.access(targetPath);
  } catch {
    return targetPath; // 目标不存在，直接使用
  }
  const dir = path.dirname(targetPath);
  const ext = path.extname(targetPath);
  const base = path.basename(targetPath, ext);
  for (let counter = 1; counter < 100; counter++) {
    const candidate = path.join(dir, `${base} (${counter})${ext}`);
    try {
      await fs.promises.access(candidate);
    } catch {
      return candidate; // 找到可用路径
    }
  }
  return targetPath; // fallback
}

export async function renameEntry(oldPath: string, newPath: string): Promise<void> {
  validatePath(oldPath);
  validatePath(newPath);

  // 检查源文件是否存在
  await fs.promises.stat(oldPath);

  // 自动递增（如果目标已存在）
  const targetPath = await deduplicatePath(newPath);

  await fs.promises.rename(oldPath, targetPath);
}

/**
 * 删除到系统回收站（macOS osascript）
 * @param filePath 文件路径
 *
 * 安全改进（对标 Natives2）：使用 POSIX file 对象传递路径，
 * 避免字符串插值导致的命令注入。
 */
export async function trashEntry(filePath: string): Promise<void> {
  validatePath(filePath);
  await fs.promises.stat(filePath); // 确保文件存在

  // 安全方式：将路径作为 POSIX file 对象传递，不经过 shell 解释
  const script = `
    use framework "Foundation"
    set fileURL to current application's NSURL's fileURLWithPath:"${filePath.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"
    set workspace to current application's NSWorkspace's sharedWorkspace()
    workspace's recycleURLs:{fileURL} completionHandler:(missing value)
  `;
  await new Promise<void>((resolve, reject) => {
    execFile('osascript', ['-l', 'AppleScript', '-e', script], { timeout: 10000 }, (err, stdout, stderr) => {
      if (err) {
        // 降级方案：使用 Finder 的 POSIX file（安全，因为路径已验证）
        execFile('osascript', [
          '-e',
          `POSIX file "${filePath.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}" as alias`,
          '-e',
          'tell application "Finder" to delete item 1 of result',
        ], { timeout: 10000 }, (err2, stdout2, stderr2) => {
          if (err2) {
            reject(Object.assign(new Error(`Failed to trash: ${stderr2 || err2.message}`), { code: 'ETRASH' }));
          } else {
            resolve();
          }
        });
      } else {
        resolve();
      }
    });
  });
}

// ── Move Entry ──

/**
 * 移动文件（同卷 rename，跨卷 copy+delete）
 * @param from 源路径
 * @param to 目标路径
 */
export async function moveEntry(from: string, to: string): Promise<void> {
  validatePath(from);
  validatePath(to);

  // 检查源是否存在
  await fs.promises.stat(from);

  // 自动递增（如果目标已存在）
  const targetPath = await deduplicatePath(to);

  try {
    // 尝试同卷 rename
    await fs.promises.rename(from, targetPath);
  } catch (err: any) {
    if (err.code === 'EXDEV') {
      // 跨卷：copy + delete
      const isDir = (await fs.promises.stat(from)).isDirectory();
      if (isDir) {
        // 递归复制目录
        await copyDir(from, targetPath);
      } else {
        await fs.promises.copyFile(from, targetPath);
      }
      // 删除源
      await fs.promises.rm(from, { recursive: true, force: true });
    } else {
      throw err;
    }
  }
}

async function copyDir(src: string, dest: string): Promise<void> {
  await fs.promises.mkdir(dest, { recursive: true });
  const entries = await fs.promises.readdir(src, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      await copyDir(srcPath, destPath);
    } else {
      await fs.promises.copyFile(srcPath, destPath);
    }
  }
}

/**
 * 导入文件到目录（从拖放/复制）
 * @param sourcePaths 源文件路径列表
 * @param destDir 目标目录
 * @returns 实际写入的文件路径列表
 */
export async function importFiles(sourcePaths: string[], destDir: string): Promise<string[]> {
  const results: string[] = [];

  for (const srcPath of sourcePaths) {
    const baseName = path.basename(srcPath);
    const destPath = await deduplicatePath(path.join(destDir, baseName));

    // 复制文件
    const stat = await fs.promises.stat(srcPath);
    if (stat.isDirectory()) {
      await copyDir(srcPath, destPath);
    } else {
      await fs.promises.copyFile(srcPath, destPath);
    }
    results.push(destPath);
  }

  return results;
}
