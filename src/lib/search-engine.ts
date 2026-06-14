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
    if (positions[i] === positions[i - 1] + 1) {
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
