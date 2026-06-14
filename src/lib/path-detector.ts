import * as path from 'path';

export interface PathMatch {
  path: string;
  line?: number;
  column?: number;
  start: number;
  end: number;
}

export function detectFilePaths(text: string, currentDir: string): PathMatch[] {
  const matches: PathMatch[] = [];

  // 1. 行号引用: file.ts:42 or file.ts:42:10 (最优先)
  const linePattern = /([\w\-.]+\.\w+):(\d+)(?::(\d+))?/g;
  let match: RegExpExecArray | null;
  while ((match = linePattern.exec(text)) !== null) {
    const resolved = path.resolve(currentDir, match[1]!);
    matches.push({
      path: resolved,
      line: parseInt(match[2]!, 10),
      column: match[3] ? parseInt(match[3]!, 10) : undefined,
      start: match.index,
      end: match.index + match[0].length,
    });
  }

  // 2. 相对路径: ./src/file.ts or ../file.ts (优先于绝对路径)
  const relPattern = /(\.\.?\/[\w\-.]+(?:\/[\w\-.]+)*\.\w+)/g;
  while ((match = relPattern.exec(text)) !== null) {
    const resolved = path.resolve(currentDir, match[1]!);
    if (!isOverlapping(matches, match.index, match.index + match[0].length)) {
      matches.push({ path: resolved, start: match.index, end: match.index + match[0].length });
    }
  }

  // 3. 绝对路径: /Users/xxx/file.ts
  const absPattern = /(\/[^\s:\"'<>&|;`!@#$%^*()+={}\[\]\?]+\.\w+)/g;
  while ((match = absPattern.exec(text)) !== null) {
    if (!isOverlapping(matches, match.index, match.index + match[0].length)) {
      matches.push({ path: match[1]!, start: match.index, end: match.index + match[0].length });
    }
  }

  return matches;
}

function isOverlapping(matches: PathMatch[], start: number, end: number): boolean {
  return matches.some((m) => m.start <= end && m.end >= start);
}
