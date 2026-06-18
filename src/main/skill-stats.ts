import { promises as fsp } from 'fs';
import * as path from 'path';
import * as os from 'os';

export interface SkillStat {
  count: number;
  lastTriggered?: number;
}

/** 最大扫描文件数（防止 I/O 风暴） */
const MAX_FILES = 500;

/** 最大扫描时间（墙钟，毫秒） */
const MAX_TIME_MS = 3_000;

/** 日志保留窗口（天） */
const RETENTION_DAYS = 45;

// ── 增量解析缓存（对标 fanbox 的 claudeFileCache） ──

interface FileCacheEntry {
  offset: number;
  lastMsgId: string;
  stats: Record<string, SkillStat>;
}

/** 文件偏移量缓存 — 跨调用复用，只解析增量内容 */
const fileCache = new Map<string, FileCacheEntry>();

/** 上次缓存清理时间 */
let lastCacheCleanup = 0;

/** 缓存清理间隔（毫秒） */
const CACHE_CLEANUP_INTERVAL = 60_000;

/**
 * 增量解析单个 JSONL 文件（对标 fanbox 的 parseClaudeFile）
 *
 * - 只读取 offset 之后的新内容
 * - 文件被截断时重置 offset
 * - 末尾不完整行留给下一轮
 * - 用 lastMsgId 去重同一条消息的重复 usage
 */
async function parseFileIncremental(file: string, stat: { size: number; mtimeMs: number }): Promise<Record<string, SkillStat>> {
  let cache = fileCache.get(file);
  if (!cache) {
    cache = { offset: 0, lastMsgId: '', stats: {} };
    fileCache.set(file, cache);
  }

  // 文件被截断重写：重来
  if (stat.size < cache.offset) {
    cache.offset = 0;
    cache.lastMsgId = '';
    cache.stats = {};
  }

  // 文件无变化：返回缓存
  if (stat.size === cache.offset) return cache.stats;

  // 只读增量部分
  const fh = await fsp.open(file, 'r');
  let chunk: string;
  try {
    const len = stat.size - cache.offset;
    const buf = Buffer.alloc(len);
    await fh.read(buf, 0, len, cache.offset);
    chunk = buf.toString('utf8');
  } finally {
    await fh.close();
  }

  // 末尾可能是写到一半的行：留给下一轮，offset 只推进到最后一个完整换行
  const lastNL = chunk.lastIndexOf('\n');
  if (lastNL === -1) return cache.stats;
  cache.offset += Buffer.byteLength(chunk.slice(0, lastNL + 1), 'utf8');

  // 解析新行
  for (const line of chunk.slice(0, lastNL).split('\n')) {
    if (!line.includes('"tool_use"') || !line.includes('"Skill"')) continue;
    try {
      const e = JSON.parse(line);
      const ts = e.timestamp ? Date.parse(e.timestamp) : 0;

      // 同一条消息分多行落盘，usage 重复：只记第一次
      const msgId = e.message?.id;
      if (msgId && msgId === cache.lastMsgId) continue;
      if (msgId) cache.lastMsgId = msgId;

      if (e.type === 'assistant' && Array.isArray(e.message?.content)) {
        for (const b of e.message.content) {
          if (b?.type === 'tool_use' && b.name === 'Skill' && b.input?.skill) {
            const name = b.input.skill;
            if (!cache.stats[name]) cache.stats[name] = { count: 0 };
            cache.stats[name].count++;
            if (ts && (!cache.stats[name].lastTriggered || ts > cache.stats[name].lastTriggered!)) {
              cache.stats[name].lastTriggered = ts;
            }
          }
        }
      }
    } catch { /* skip malformed lines */ }
  }

  return cache.stats;
}

/**
 * 清理过期文件缓存（对标 fanbox 的 cache eviction）
 */
function evictStaleCache(liveFiles: Set<string>): void {
  for (const key of fileCache.keys()) {
    if (!liveFiles.has(key)) fileCache.delete(key);
  }
}

/**
 * 扫描 Claude Code 会话日志，统计每个 Skill 的触发次数
 *
 * 增量解析（对标 fanbox）：
 * - 跨调用复用文件偏移量缓存，只解析新增内容
 * - 文件被截断时自动重置
 * - 末尾不完整行留给下一轮
 * - 带文件数/时间上限，防止大量日志导致 I/O 风暴
 */
export async function getSkillStats(): Promise<Record<string, SkillStat>> {
  const projectsDir = path.join(os.homedir(), '.claude', 'projects');
  const now = Date.now();
  const cutoff = now - RETENTION_DAYS * 86400_000;
  const deadline = now + MAX_TIME_MS;

  // 收集所有日志文件（带 stat 信息）
  const files: { fp: string; size: number; mtimeMs: number }[] = [];
  try {
    for (const dir of await fsp.readdir(projectsDir, { withFileTypes: true })) {
      if (!dir.isDirectory()) continue;
      const sub = path.join(projectsDir, dir.name);
      try {
        for (const f of await fsp.readdir(sub)) {
          if (!f.endsWith('.jsonl')) continue;
          const fp = path.join(sub, f);
          try {
            const st = await fsp.stat(fp);
            if (st.mtimeMs >= cutoff) {
              files.push({ fp, size: st.size, mtimeMs: st.mtimeMs });
            }
          } catch { /* skip unreadable files */ }
        }
      } catch { /* skip unreadable subdirs */ }
    }
  } catch {
    return {}; // 无日志目录，返回空（诚实，不造假）
  }

  // 清理过期缓存（定期执行，非每次）
  if (now - lastCacheCleanup > CACHE_CLEANUP_INTERVAL) {
    evictStaleCache(new Set(files.map((f) => f.fp)));
    lastCacheCleanup = now;
  }

  // 增量解析所有文件
  let fileCount = 0;
  const merged: Record<string, SkillStat> = {};

  for (const { fp, size, mtimeMs } of files) {
    if (Date.now() > deadline) break;
    if (fileCount >= MAX_FILES) break;
    fileCount++;

    try {
      const fileStats = await parseFileIncremental(fp, { size, mtimeMs });
      for (const [name, stat] of Object.entries(fileStats)) {
        if (!merged[name]) merged[name] = { count: 0 };
        merged[name].count += stat.count;
        if (stat.lastTriggered && (!merged[name].lastTriggered || stat.lastTriggered > merged[name].lastTriggered!)) {
          merged[name].lastTriggered = stat.lastTriggered;
        }
      }
    } catch { /* skip unreadable files */ }
  }

  return merged;
}
