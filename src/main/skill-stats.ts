import { promises as fsp } from 'fs';
import * as path from 'path';
import * as os from 'os';

export interface SkillStat {
  count: number;
  lastTriggered?: number;
}

/**
 * 扫描 Claude Code 会话日志，统计每个 Skill 的触发次数
 *
 * 遍历 ~/.claude/projects/ 下所有 .jsonl，从 assistant.message.content[] 中
 * 匹配 name=='Skill' 的 tool_use 事件，按 skill 名称聚合。
 * 只扫描最近 45 天的日志文件（PRD M15）。
 */
export async function getSkillStats(): Promise<Record<string, SkillStat>> {
  const stats: Record<string, SkillStat> = {};
  const projectsDir = path.join(os.homedir(), '.claude', 'projects');
  let files: string[] = [];
  try {
    for (const dir of await fsp.readdir(projectsDir, { withFileTypes: true })) {
      if (!dir.isDirectory()) continue;
      const sub = path.join(projectsDir, dir.name);
      try {
        for (const f of await fsp.readdir(sub)) {
          if (f.endsWith('.jsonl')) files.push(path.join(sub, f));
        }
      } catch { /* skip unreadable subdirs */ }
    }
  } catch { return stats; } // 无日志目录，返回空（诚实，不造假）

  const now = Date.now();
  for (const file of files) {
    try {
      const st = await fsp.stat(file);
      if (now - st.mtimeMs > 45 * 86400_000) continue; // 只扫最近 45 天
    } catch { continue; }
    try {
      const content = await fsp.readFile(file, 'utf-8');
      for (const line of content.split('\n')) {
        if (!line.trim()) continue;
        try {
          const e = JSON.parse(line);
          const ts = e.timestamp ? Date.parse(e.timestamp) : 0;
          if (e.type === 'assistant' && Array.isArray(e.message?.content)) {
            for (const b of e.message.content) {
              if (b?.type === 'tool_use' && b.name === 'Skill' && b.input?.skill) {
                const name = b.input.skill;
                if (!stats[name]) stats[name] = { count: 0 };
                stats[name].count++;
                if (ts && (!stats[name].lastTriggered || ts > stats[name].lastTriggered!)) {
                  stats[name].lastTriggered = ts;
                }
              }
            }
          }
        } catch { /* skip malformed lines */ }
      }
    } catch { /* skip unreadable files */ }
  }
  return stats;
}
