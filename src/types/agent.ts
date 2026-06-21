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
  | 'residue-files'
  | 'residue-file';

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
  /** description 原始长度（截断前） */
  descLen?: number;
  source: SkillSource;
  /** 来源标签（如 "~/.claude"、"my-project"） */
  label?: string;
  path: string;
  /** skill 目录路径 */
  dir?: string;
  enabled: boolean;
  /** 是否为残留文件（非有效 skill） */
  residue?: boolean;
  health: SkillHealth;
  /** 触发次数 */
  triggerCount?: number;
  /** 命中次数（调用次数，新字段名） */
  hitCount?: number;
  /** 最后触发时间（epoch ms） */
  lastTriggered?: number;
  /** 跨来源副本：同名 skill 出现在多处时，列出各来源路径 */
  copies?: string[];
  /** 修改时间（epoch ms） */
  mtime?: number;
  /** 技能图标 URL（可选） */
  icon?: string;
  /** 技能 ID（唯一标识） */
  id?: string;
}

/** Skills 概览统计 */
export interface SkillsOverview {
  total: number;
  unique: number;
  active: number;
  dust: number;
  issues: number;
  budgetChars: number;
  budgetLimit: number;
  descCut: number;
}

/** Skills 扫描完整数据 */
export interface SkillsData {
  ok: boolean;
  at: number;
  items: SkillInfo[];
  overview: SkillsOverview;
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
    /** 输入 Token */
    input?: number;
    /** 输出 Token */
    output?: number;
    /** 缓存创建 Token */
    cacheCreation?: number;
    /** 缓存命中 Token */
    cacheRead?: number;
  };
  /** 活跃统计 */
  activity: {
    totalSessions: number;
    totalMessages: number;
    firstSessionDate: string;
  };
  /** 总请求数 */
  totalRequests?: number;
  /** 总成本（美元） */
  totalCost?: number | null;
}

/** Token 使用历史数据点（用于趋势图） */
export interface UsageHistoryPoint {
  timestamp: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  skills?: number;
}

/** Codex 用量（来源：~/.codex/account.json + sessions） */
export interface CodexUsage {
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  todayTokens: number;
  weekTokens: number;
  totalSessions: number;
  totalCost: number;
  models: Record<string, ModelTokenUsage>;
  history: UsageHistoryPoint[];
}

/** RTK CLI 代理命令统计 */
export interface RtkCommandHistory {
  command: string;
  timestamp: number;
  tokensSaved: number;
}

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

/** 用量响应（匹配后端 UsageResponse） */
export interface UsageResponse {
  claude: ClaudeUsage | null;
  codex: CodexUsage | null;
  rtk: RtkUsage | null;
  history: UsageHistoryPoint[];
  modelStats: ModelStat[];
  /** 是否有真实数据源（Claude/Codex/RTK） */
  sourceConfigured: boolean;
  /** 源面包屑：每个数据来源的文件路径 */
  sourceBreadcrumbs: string[];
  /** 错误信息 */
  error?: string | null;
}

/** 模型统计（待后端返回后渲染） */
export interface ModelStat {
  model: string;
  requestCount: number;
  totalTokens: number;
  totalCost: number;
  avgCostPerRequest: number;
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

/** 文件变更事件 */
export interface FileChangeEvent {
  path: string;
  type: 'create' | 'modify' | 'delete';
  timestamp: number;
  sessionId?: string;
}
