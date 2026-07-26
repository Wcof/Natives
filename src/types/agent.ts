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

/** 会话信息 — 与 src-tauri/src/agent.rs SessionInfo（camelCase）对齐 */
export interface AgentSession {
  /** 会话 UUID（可直接 `claude --resume <id>`） */
  id: string;
  /** jsonl 转写文件绝对路径 */
  path: string;
  /** 最后活动时间（epoch ms） */
  mtimeMs: number;
  /** 转写文件大小（字节） */
  size: number;
  /** 会话摘要（可能缺失） */
  title?: string | null;
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

/** 文件变更事件 — 来源为真实 fs-watch-change 管道（fsWatch.onChange） */
export interface FileChangeEvent {
  path: string;
  type: 'create' | 'modify' | 'delete';
  timestamp: number;
}
