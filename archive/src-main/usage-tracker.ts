import { type ClaudeUsage, type RtkUsage } from '../types/agent';

// ── Helpers ──

/** 获取本地日期字符串 YYYY-MM-DD */
function localDateStr(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
}

/** 获取本周一的日期字符串 */
function getWeekStartStr(): string {
  const now = new Date();
  const day = now.getDay();
  const diff = day === 0 ? 6 : day - 1; // Monday = 0
  const monday = new Date(now);
  monday.setDate(now.getDate() - diff);
  return localDateStr(monday);
}

// ── Claude Code Usage（来源：~/.claude/stats-cache.json） ──

/**
 * 解析 Claude Code 的 stats-cache.json
 *
 * 数据结构：
 * {
 *   modelUsage: { [modelId]: { inputTokens, outputTokens, cacheReadInputTokens, ... } },
 *   dailyModelTokens: [ { date: "YYYY-MM-DD", tokensByModel: { [modelId]: number } } ],
 *   totalSessions: number,
 *   totalMessages: number,
 *   firstSessionDate: string,
 * }
 *
 * 不伪造 5h 窗口或周配额 — 这些是 API 级别的速率限制，stats-cache.json 不包含此数据。
 * 只展示真实存在的本地统计数据。
 */
export function parseClaudeStatsCache(data: any): ClaudeUsage | null {
  if (!data || typeof data !== 'object') return null;

  const models: ClaudeUsage['models'] = {};
  const modelUsage = data.modelUsage;
  if (modelUsage && typeof modelUsage === 'object') {
    for (const [modelId, usage] of Object.entries(modelUsage)) {
      const u = usage as any;
      models[modelId] = {
        inputTokens: u.inputTokens || 0,
        outputTokens: u.outputTokens || 0,
        cacheReadInputTokens: u.cacheReadInputTokens || 0,
        cacheCreationInputTokens: u.cacheCreationInputTokens || 0,
        costUSD: u.costUSD || 0,
      };
    }
  }

  // 从 dailyModelTokens 聚合本地 token 统计
  const todayStr = localDateStr(new Date());
  const weekStartStr = getWeekStartStr();
  let todayTokens = 0;
  let weekTokens = 0;
  let totalTokens = 0;

  const dailyModelTokens = data.dailyModelTokens;
  if (Array.isArray(dailyModelTokens)) {
    for (const entry of dailyModelTokens) {
      if (!entry?.date || !entry.tokensByModel) continue;
      let dayTotal = 0;
      for (const tokens of Object.values(entry.tokensByModel)) {
        dayTotal += (tokens as number) || 0;
      }
      totalTokens += dayTotal;
      if (entry.date === todayStr) todayTokens = dayTotal;
      if (entry.date >= weekStartStr) weekTokens += dayTotal;
    }
  }

  return {
    models,
    localTokens: {
      today: todayTokens,
      thisWeek: weekTokens,
      total: totalTokens,
    },
    activity: {
      totalSessions: data.totalSessions || 0,
      totalMessages: data.totalMessages || 0,
      firstSessionDate: data.firstSessionDate || '',
    },
  };
}

// ── RTK Usage ──

export function parseRtkUsage(output: string): RtkUsage {
  const lines = output.split('\n').filter(Boolean);
  const history: RtkUsage['history'] = [];
  const commandCounts = new Map<string, { count: number; totalSaved: number }>();

  let totalSaved = 0;
  let totalCommands = 0;

  for (const line of lines) {
    // Format: command | tokens_saved | timestamp
    const parts = line.split('|').map((s) => s.trim());
    if (parts.length >= 2) {
      const command = parts[0]!;
      const saved = parseInt(parts[1]!, 10) || 0;
      const timestamp = parts[2] ? parseInt(parts[2]!, 10) : Date.now();

      totalSaved += saved;
      totalCommands++;

      history.push({ command, timestamp, tokensSaved: saved });

      const existing = commandCounts.get(command) || { count: 0, totalSaved: 0 };
      existing.count++;
      existing.totalSaved += saved;
      commandCounts.set(command, existing);
    }
  }

  const topCommands = [...commandCounts.entries()]
    .map(([command, stats]) => ({ command, ...stats }))
    .sort((a, b) => b.totalSaved - a.totalSaved)
    .slice(0, 10);

  return { totalSaved, totalCommands, history, topCommands };
}
