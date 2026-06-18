// ── Agent Types ──

/** Agent 引擎类型 */
export type AgentEngine = 'claude' | 'codex';

/** Agent 状态 */
export type AgentStatus = 'running' | 'idle' | 'exited';

/** Skill 来源目录 */
export type SkillSource =
  | '~/.claude/skills'
  | 'project/.claude/skills'
  | 'claude-plugins'
  | '~/.codex/skills'
  | '~/.agents/skills';

/** Skill 问题类型 */
export type SkillIssue =
  | 'description-truncated'
  | 'missing-frontmatter'
  | 'missing-skill-md'
  | 'residue-files';

// ── Interfaces ──

/** Agent 项目 */
export interface AgentProject {
  path: string;
  name: string;
  engine: AgentEngine;
  lastActive: number;
  sessionCount: number;
}

/** 会话信息 */
export interface AgentSession {
  id: string;
  engine: AgentEngine;
  projectPath: string;
  title: string;
  startTime: number;
  endTime?: number;
  filesModified: string[];
  fileTimestamps: Record<string, number>;
  skillsUsed: string[];
}

/** Skill 信息 */
export interface SkillInfo {
  name: string;
  description: string;
  source: SkillSource;
  path: string;
  enabled: boolean;
  health: SkillHealth;
  triggerCount: number;
  lastTriggered?: number;
}

/** Skill 健康状态 */
export interface SkillHealth {
  ok: boolean;
  issues: SkillIssue[];
}

/** 单模型 token 统计（来源：~/.claude/stats-cache.json modelUsage） */
export interface ModelTokenUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  costUSD: number;
}

/** Claude Code 用量（来源：~/.claude/stats-cache.json，无伪造数据） */
export interface ClaudeUsage {
  /** 各模型 token 用量明细 */
  models: Record<string, ModelTokenUsage>;
  /** 本地统计 token（从 stats-cache.json dailyModelTokens 聚合） */
  localTokens: {
    today: number;
    thisWeek: number;
    total: number;
  };
  /** 活跃统计 */
  activity: {
    totalSessions: number;
    totalMessages: number;
    firstSessionDate: string;
  };
}

/** Codex 用量 */
export interface CodexUsage {
  fiveHourWindow: {
    used: number;
    limit: number;
    resetAt: number;
  };
  planType: string;
}

/** RTK 命令历史 */
export interface RtkCommandHistory {
  command: string;
  timestamp: number;
  tokensSaved: number;
}

/** RTK 命令统计 */
export interface RtkCommandStat {
  command: string;
  count: number;
  totalSaved: number;
}

/** RTK 用量 */
export interface RtkUsage {
  totalSaved: number;
  totalCommands: number;
  history: RtkCommandHistory[];
  topCommands: RtkCommandStat[];
}

/** 文件变更事件 */
export interface FileChangeEvent {
  path: string;
  type: 'create' | 'modify' | 'delete';
  timestamp: number;
  sessionId?: string;
}

// ── Constants ──

/** 所有 Skill 来源 */
export const SKILL_SOURCES: SkillSource[] = [
  '~/.claude/skills',
  'project/.claude/skills',
  'claude-plugins',
  '~/.codex/skills',
  '~/.agents/skills',
];

/** 所有可能的 Skill 问题 */
export const SKILL_ISSUES: SkillIssue[] = [
  'description-truncated',
  'missing-frontmatter',
  'missing-skill-md',
  'residue-files',
];
