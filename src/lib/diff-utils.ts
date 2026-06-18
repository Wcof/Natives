/**
 * Shared diff parsing utilities.
 */

export function parseUnifiedDiff(diff: string): { original: string; modified: string } | null {
  const lines = diff.split('\n');
  const originalLines: string[] = [];
  const modifiedLines: string[] = [];
  let inHunk = false;

  for (const line of lines) {
    if (line.startsWith('diff ') || line.startsWith('index ') || line.startsWith('--- ') || line.startsWith('+++ ')) continue;
    if (line.startsWith('@@')) { inHunk = true; continue; }
    if (!inHunk) continue;

    if (line.startsWith('-')) {
      originalLines.push(line.slice(1));
    } else if (line.startsWith('+')) {
      modifiedLines.push(line.slice(1));
    } else {
      originalLines.push(line);
      modifiedLines.push(line);
    }
  }

  if (originalLines.length === 0 && modifiedLines.length === 0) return null;
  return {
    original: originalLines.join('\n'),
    modified: modifiedLines.join('\n'),
  };
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
