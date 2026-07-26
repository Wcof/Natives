/**
 * fs-change-filter — 文件变更事件的噪声过滤（纯函数，便于单测）
 *
 * 规则移植自 fanbox（References/fanbox/public/app.js isNoisyChange/selfOpened）：
 * - 相对被监听根目录的路径中，任何以 `.` 开头的段（.git/.DS_Store/.cache…）
 * - 构建/依赖目录段（node_modules、dist、target…）
 * - 编辑器/下载器/数据库的临时文件后缀；`.tmp` 可能出现在文件名中段
 *   （如 `foo.swift.tmp.<pid>.<hex>`），故按段匹配而非仅结尾
 * - 自己刚打开的文件 3 秒内的变更（macOS LaunchServices 打开文件会写
 *   `com.apple.lastuseddate#PS` 扩展属性，触发假 modify 事件）
 */

const IGNORE_DIR_SEGMENTS = new Set([
  'node_modules', 'dist', 'build', 'out', 'target',
  '__pycache__', 'venv', 'Pods', 'DerivedData', 'coverage',
]);

const NOISY_SUFFIX_RE = /(~|\.swp|\.swo|\.swx|\.part|\.partial|\.crdownload|\.download|\.lock|-journal|-shm|-wal)$/i;
const TMP_SEGMENT_RE = /(^|\.)tmp(\.|$)/i;

/**
 * 判断一条变更路径是否为噪声。
 * 传入 `root`（被监听目录）时只检查相对部分的段——这样监听
 * `~/.config/foo` 这类本身含点段的目录时，内部变更不会被误杀。
 */
export function isNoisyChangePath(path: string, root?: string): boolean {
  let rel = path;
  if (root) {
    const r = root.endsWith('/') ? root.slice(0, -1) : root;
    if (path === r) return false;
    if (path.startsWith(r + '/')) rel = path.slice(r.length + 1);
  }
  const segments = rel.split('/').filter(Boolean);
  if (segments.length === 0) return false;
  for (const seg of segments) {
    if (seg.startsWith('.')) {
      // 隐藏段整体视为噪声（.git 内部写入、.DS_Store、.cache…）
      return true;
    }
    if (IGNORE_DIR_SEGMENTS.has(seg)) return true;
  }
  const name = segments[segments.length - 1]!;
  if (NOISY_SUFFIX_RE.test(name)) return true;
  if (TMP_SEGMENT_RE.test(name)) return true;
  return false;
}

/**
 * 把变更路径映射到被监听根目录的直接子项（用于点亮对应卡片）。
 * 深层写入归并到顶层子目录：`/root/src/a/b.ts` → `/root/src`。
 * 不在根目录下返回 null。
 */
export function topChildOf(root: string, path: string): string | null {
  const r = root.endsWith('/') ? root.slice(0, -1) : root;
  if (!path.startsWith(r + '/')) return null;
  const rest = path.slice(r.length + 1);
  const seg = rest.split('/')[0];
  return seg ? `${r}/${seg}` : null;
}

/** 自己刚打开的文件登记表；窗口期内的变更事件整条丢弃。 */
export class SelfOpenedTracker {
  private opened = new Map<string, number>();

  constructor(
    private windowMs = 3000,
    private maxEntries = 64,
  ) {}

  mark(path: string, now: number = Date.now()): void {
    this.opened.set(path, now);
    if (this.opened.size > this.maxEntries) {
      // Map 迭代按插入序，删最旧的一条即可控制上限
      const oldest = this.opened.keys().next().value;
      if (oldest !== undefined) this.opened.delete(oldest);
    }
  }

  isSelfNoise(path: string, now: number = Date.now()): boolean {
    const at = this.opened.get(path);
    if (at === undefined) return false;
    if (now - at < this.windowMs) return true;
    this.opened.delete(path);
    return false;
  }
}
