/**
 * path-utils — 纯 TS 的路径工具(替代 Renderer 中的 Node `path` builtin, P1-013)。
 *
 * Renderer 生产 bundle 禁止依赖 Node `path/fs/child_process`;这里实现
 * path-detector / search-engine 等模块实际使用的最小 path 子集
 * (resolve / isAbsolute / dirname / basename / extname / join / normalize),
 * 语义与 Node path(posix 风格)对齐,但不触文件系统。
 */

const SEP = '/';

export function isAbsolute(p: string): boolean {
  return p.startsWith('/');
}

/** 折叠 ./ 与 ../,返回规范化路径(不解析 symlink,不触 FS)。 */
export function normalize(p: string): string {
  const isAbs = p.startsWith('/');
  const parts = p.split(SEP);
  const out: string[] = [];
  for (const seg of parts) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') {
      if (out.length > 0 && out[out.length - 1] !== '..') out.pop();
      else if (!isAbs) out.push('..');
      continue;
    }
    out.push(seg);
  }
  const joined = out.join(SEP);
  if (isAbs) return '/' + joined;
  return joined === '' ? '.' : joined;
}

/** 以 currentDir 为基准解析 to(绝对路径原样返回)。 */
export function resolve(currentDir: string, to: string): string {
  if (isAbsolute(to)) return normalize(to);
  const base = isAbsolute(currentDir) ? currentDir : normalize(currentDir);
  if (base === '/') return normalize('/' + to);
  return normalize(base + SEP + to);
}

export function dirname(p: string): string {
  if (!p) return '.';
  const norm = p.endsWith(SEP) ? p.slice(0, -1) : p;
  const idx = norm.lastIndexOf(SEP);
  if (idx <= 0) return idx === 0 ? '/' : '.';
  return norm.slice(0, idx);
}

export function basename(p: string): string {
  if (!p) return '';
  const norm = p.endsWith(SEP) ? p.slice(0, -1) : p;
  const idx = norm.lastIndexOf(SEP);
  return idx < 0 ? norm : norm.slice(idx + 1);
}

export function extname(p: string): string {
  const base = basename(p);
  const dot = base.lastIndexOf('.');
  if (dot <= 0) return '';
  return base.slice(dot);
}

export function join(...parts: string[]): string {
  const nonEmpty = parts.filter((p) => p.length > 0);
  if (nonEmpty.length === 0) return '.';
  const isAbs = isAbsolute(nonEmpty[0]!);
  const joined = nonEmpty.join(SEP);
  return isAbs ? normalize('/' + joined.replace(/^\/+/, '')) : normalize(joined);
}
