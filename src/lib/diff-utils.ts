/**
 * Shared diff parsing utilities.
 * Includes real line-diff (handles inserts without line-number skew).
 */

export interface DiffHunkLine {
  kind: 'context' | 'add' | 'del';
  text: string;
  oldNo: number | null;
  newNo: number | null;
}

export interface DiffHunk {
  header: string;
  lines: DiffHunkLine[];
}

export interface DiffResult {
  hunks: DiffHunk[];
  /** e.g. "+3 −1" */
  diffstat: string;
  additions: number;
  deletions: number;
  original: string;
  modified: string;
}

/**
 * LCS-based line diff → unified-style hunks.
 * Unlike index-aligned comparison, inserts do not shift subsequent pairings incorrectly.
 */
export function computeLineDiff(oldContent: string, newContent: string, context = 3): DiffResult {
  const a = oldContent.length ? oldContent.split('\n') : [];
  const b = newContent.length ? newContent.split('\n') : [];
  // Drop trailing empty from final newline symmetry
  if (a.length && a[a.length - 1] === '') a.pop();
  if (b.length && b[b.length - 1] === '') b.pop();

  const n = a.length;
  const m = b.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i]![j] = a[i] === b[j] ? (dp[i + 1]![j + 1]! + 1) : Math.max(dp[i + 1]![j]!, dp[i]![j + 1]!);
    }
  }

  type Op = { type: 'eq' | 'add' | 'del'; text: string; oldNo: number | null; newNo: number | null };
  const ops: Op[] = [];
  let i = 0;
  let j = 0;
  let oldNo = 1;
  let newNo = 1;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      ops.push({ type: 'eq', text: a[i]!, oldNo: oldNo++, newNo: newNo++ });
      i++;
      j++;
    } else if (dp[i + 1]![j]! >= dp[i]![j + 1]!) {
      ops.push({ type: 'del', text: a[i]!, oldNo: oldNo++, newNo: null });
      i++;
    } else {
      ops.push({ type: 'add', text: b[j]!, oldNo: null, newNo: newNo++ });
      j++;
    }
  }
  while (i < n) {
    ops.push({ type: 'del', text: a[i++]!, oldNo: oldNo++, newNo: null });
  }
  while (j < m) {
    ops.push({ type: 'add', text: b[j++]!, oldNo: null, newNo: newNo++ });
  }

  const changeIdx = ops
    .map((op, idx) => (op.type !== 'eq' ? idx : -1))
    .filter((idx) => idx >= 0);

  const hunks: DiffHunk[] = [];
  if (changeIdx.length === 0) {
    return {
      hunks: [],
      diffstat: '+0 −0',
      additions: 0,
      deletions: 0,
      original: a.join('\n'),
      modified: b.join('\n'),
    };
  }

  let cursor = 0;
  while (cursor < changeIdx.length) {
    let start = Math.max(0, changeIdx[cursor]! - context);
    let end = Math.min(ops.length - 1, changeIdx[cursor]! + context);
    let k = cursor + 1;
    while (k < changeIdx.length && changeIdx[k]! <= end + context) {
      end = Math.min(ops.length - 1, changeIdx[k]! + context);
      k++;
    }
    // expand start for contiguous group
    start = Math.max(0, changeIdx[cursor]! - context);
    const slice = ops.slice(start, end + 1);
    const oldStart = slice.find((s) => s.oldNo != null)?.oldNo ?? 0;
    const newStart = slice.find((s) => s.newNo != null)?.newNo ?? 0;
    const oldCount = slice.filter((s) => s.type !== 'add').length;
    const newCount = slice.filter((s) => s.type !== 'del').length;
    hunks.push({
      header: `@@ -${oldStart},${oldCount} +${newStart},${newCount} @@`,
      lines: slice.map((s) => ({
        kind: s.type === 'eq' ? 'context' : s.type === 'add' ? 'add' : 'del',
        text: s.text,
        oldNo: s.oldNo,
        newNo: s.newNo,
      })),
    });
    cursor = k;
  }

  const additions = ops.filter((o) => o.type === 'add').length;
  const deletions = ops.filter((o) => o.type === 'del').length;
  return {
    hunks,
    diffstat: `+${additions} −${deletions}`,
    additions,
    deletions,
    original: a.join('\n'),
    modified: b.join('\n'),
  };
}

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
