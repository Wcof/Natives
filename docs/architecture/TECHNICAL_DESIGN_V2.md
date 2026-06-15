# Natives v2.0 — 新增模块技术设计方案

> **版本**: 1.0.0
> **日期**: 2026-06-14
> **状态**: Draft
> **范围**: M14-M18 新增模块 + 终端增强 + 主题精简

---

## 目录

1. [M14: File Manager](#m14-file-manager)
2. [M14.1: File Search Engine](#m141-file-search-engine)
3. [M14.2: Thumbnail Generator](#m142-thumbnail-generator)
4. [M14.3: Git Integration](#m143-git-integration)
5. [M14.4: Disk Usage Analyzer](#m144-disk-usage-analyzer)
6. [M15: Agent Integration](#m15-agent-integration)
7. [M15.1: Session Scanner](#m151-session-scanner)
8. [M15.2: Skills Manager](#m152-skills-manager)
9. [M15.3: Usage Tracker](#m153-usage-tracker)
10. [M15.4: File Watcher](#m154-file-watcher)
11. [M16: Screenshot & Annotation](#m16-screenshot--annotation)
12. [M17: Release Wizard](#m17-release-wizard)
13. [M18: Update Checker](#m18-update-checker)
14. [Terminal Enhancement](#terminal-enhancement)
15. [Theme Simplification](#theme-simplification)
16. [Security Hardening](#security-hardening)
17. [Frontend Components](#frontend-components)

---

## M14: File Manager

### 职责

文件系统操作的核心模块，提供目录列表、文件读写、文件操作、搜索等能力。

### 数据结构

```typescript
// 文件条目
interface FileEntry {
  name: string;           // 文件名
  path: string;           // 完整路径
  isDir: boolean;         // 是否目录
  kind: FileKind;         // 文件类型
  hidden: boolean;         // 是否隐藏文件
  size: number;           // 文件大小（字节）
  mtime: number;          // 修改时间（Unix ms）
  btime: number;          // 创建时间（Unix ms）
  projectBadge?: ProjectBadge; // 项目类型徽章
  symlink?: string;       // 符号链接目标
}

// 文件类型
type FileKind = 'text' | 'image' | 'video' | 'audio' | 'pdf' | 'archive' | 'other';

// 项目徽章
type ProjectBadge = 'node' | 'web' | 'python' | 'rust' | 'go' | 'git';

// 搜索结果
interface SearchResult {
  path: string;
  name: string;
  score: number;          // 匹配分数
  isDir: boolean;
  mtime: number;
  matchRanges: [number, number][]; // 匹配字符位置
}

// 内容搜索结果
interface ContentSearchResult {
  path: string;
  name: string;
  line: number;
  preview: string;        // 匹配行的预览文本
  matchStart: number;
  matchEnd: number;
}

// Git 状态
interface GitStatus {
  root: string;           // 仓库根目录
  branch: string;         // 当前分支
  files: GitFileStatus[];
}

interface GitFileStatus {
  path: string;
  status: 'M' | 'A' | 'D' | 'R' | '??' | 'UU'; // 修改/新增/删除/重命名/未跟踪/冲突
  oldPath?: string;       // 重命名时的旧路径
}

// 磁盘用量
interface DiskUsageItem {
  name: string;
  path: string;
  isDir: boolean;
  size: number;           // 字节
  sizeFormatted: string;  // 人类可读
}

// 终端路径候选
interface PathCandidate {
  path: string;
  exists: boolean;
}
```

### API 签名

```typescript
// === 目录操作 ===

/**
 * 列出目录内容
 * @param dirPath 目录路径
 * @param options 排序/过滤选项
 * @returns 文件条目列表
 */
function listDir(dirPath: string, options?: {
  sortBy?: 'name' | 'mtime' | 'size';
  sortDir?: 'asc' | 'desc';
  showHidden?: boolean;
}): Promise<FileEntry[]>;

// === 文件读取 ===

/**
 * 读取文件内容（文本文件 ≤2MB，大文件取前 256KB）
 * @param filePath 文件路径
 * @returns 文件内容 + 元数据
 */
function readFile(filePath: string): Promise<{
  content: string;
  truncated: boolean;
  size: number;
  encoding: string;
}>;

/**
 * 流式读取文件（支持 Range 请求）
 * @param filePath 文件路径
 * @param range 可选的字节范围
 * @returns 可读流 + Content-Length + Content-Range
 */
function streamFile(filePath: string, range?: {
  start: number;
  end?: number;
}): Promise<{
  stream: ReadableStream;
  totalSize: number;
  contentRange?: string;
  contentType: string;
}>;

// === 文件写入 ===

/**
 * 原子写入文件（临时文件 + fsync + rename）
 * @param filePath 目标路径
 * @param content 文件内容
 * @param expectedMtime 可选的期望 mtime（冲突检测）
 * @returns 新的 mtime
 */
function writeFileAtomic(filePath: string, content: string, expectedMtime?: number): Promise<{
  mtime: number;
  conflict: boolean;      // mtime 不匹配时为 true
}>;

// === 文件操作 ===

/**
 * 创建文件或目录
 * @param targetPath 目标路径
 * @param type 'file' | 'dir'
 */
function createEntry(targetPath: string, type: 'file' | 'dir'): Promise<void>;

/**
 * 重命名文件或目录
 * @param oldPath 旧路径
 * @param newPath 新路径
 */
function renameEntry(oldPath: string, newPath: string): Promise<void>;

/**
 * 删除到系统回收站
 * @param filePath 文件路径
 */
function trashEntry(filePath: string): Promise<void>;

/**
 * 移动文件（同卷 rename，跨卷 copy+delete）
 * @param from 源路径
 * @param to 目标路径
 */
function moveEntry(from: string, to: string): Promise<void>;

// === 缩略图 ===

/**
 * 生成缩略图
 * @param filePath 文件路径
 * @param width 目标宽度（48-1600px）
 * @returns 缩略图 Buffer + Content-Type
 */
function generateThumb(filePath: string, width: number): Promise<{
  buffer: Buffer;
  contentType: string;
  cached: boolean;
}>;

// === 搜索 ===

/**
 * 模糊文件名搜索（评分算法）
 * @param query 搜索词
 * @param root 搜索根目录
 * @param options 选项
 * @returns 排序后的搜索结果
 */
function searchFiles(query: string, root: string, options?: {
  maxResults?: number;    // 默认 80
  includeDirs?: boolean;
  recencyBonus?: boolean;
}): Promise<SearchResult[]>;

/**
 * 全文内容搜索
 * @param query 搜索词
 * @param root 搜索根目录
 * @param options 选项
 * @returns 匹配结果
 */
function grepContent(query: string, root: string, options?: {
  maxResults?: number;
  fileExtensions?: string[];
  contextLines?: number;
}): Promise<ContentSearchResult[]>;

/**
 * macOS Spotlight 搜索
 * @param query 搜索词
 * @param root 搜索根目录
 * @returns 匹配结果
 */
function spotlightSearch(query: string, root: string): Promise<ContentSearchResult[]>;

// === Git ===

/**
 * 获取 Git 状态
 * @param dirPath 目录路径
 * @returns Git 状态信息
 */
function getGitStatus(dirPath: string): Promise<GitStatus | null>;

/**
 * 获取文件的 Git diff
 * @param filePath 文件路径
 * @returns diff 文本
 */
function getGitDiff(filePath: string): Promise<string | null>;

// === 磁盘用量 ===

/**
 * 获取目录下各项的磁盘用量
 * @param dirPath 目录路径
 * @returns 用量列表
 */
function getDiskUsage(dirPath: string): Promise<DiskUsageItem[]>;

// === 终端路径定位 ===

/**
 * 从终端输出中定位文件路径
 * @param terminalOutput 终端输出文本
 * @param currentDir 当前目录
 * @param knownRoots 已知的项目根目录列表
 * @returns 路径候选列表（按可能性排序）
 */
function locateFromTerminal(terminalOutput: string, currentDir: string, knownRoots: string[]): Promise<PathCandidate[]>;

/**
 * 批量验证路径是否存在
 * @param paths 路径列表
 * @returns 存在的路径列表
 */
function verifyPaths(paths: string[]): Promise<string[]>;
```

### 实现要点

1. **零依赖**：仅使用 Node.js 内置模块（`fs`, `path`, `os`, `crypto`, `child_process`）
2. **缩略图缓存**：`~/.natives/thumbs/` 目录，按文件路径 hash 命名，400MB 上限，LRU 淘汰
3. **文件类型检测**：基于扩展名的 40+ 文本扩展名映射表
4. **项目徽章**：检测 `package.json`（node）、`index.html`（web）、`requirements.txt`（python）、`Cargo.toml`（rust）、`go.mod`（go）、`.git`（git）
5. **中文排序**：`localeCompare` + `numeric: true`
6. **.DS_Store 过滤**：自动跳过 macOS 系统文件

### 错误处理

| 错误场景 | 处理方式 |
|----------|----------|
| 路径不存在 | 返回 ENOENT 错误，前端显示"文件不存在" |
| 权限不足 | 返回 EACCES 错误，前端显示"权限不足" |
| 磁盘空间不足 | 写入前检查，空间不足时提示用户 |
| mtime 冲突 | 返回 conflict: true，前端提示"文件已被修改，是否覆盖？" |
| 缩略图生成失败 | 返回默认图标，不阻塞 UI |

---

## M14.1: File Search Engine

### 评分算法

```typescript
function calculateScore(query: string, filename: string): number {
  let score = 0;

  // 1. 子序列匹配基础分
  const match = findSubsequence(query, filename);
  if (!match) return -1; // 不匹配

  // 2. 连续匹配奖励（streak bonus）
  const streaks = findStreaks(match);
  score += streaks.reduce((sum, s) => sum + s * s, 0) * 10;

  // 3. 词边界奖励（word boundary bonus）
  const wordBoundaries = countWordBoundaryMatches(match, filename);
  score += wordBoundaries * 15;

  // 4. 位置奖励（earlier = better）
  const firstMatchPos = match[0];
  score += Math.max(0, 10 - firstMatchPos);

  // 5. 长度惩罚（shorter filename = better）
  score -= filename.length * 0.5;

  // 6. 目录奖励
  // 调用方传入 isDir，目录额外 +5

  // 7. 最近修改奖励（recency bonus）
  const hoursSinceModified = (Date.now() - mtime) / (1000 * 60 * 60);
  score += Math.max(0, 10 - hoursSinceModified * 0.1);

  return score;
}
```

### 子序列匹配

```typescript
function findSubsequence(query: string, target: string): number[] | null {
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
```

### 连续匹配（Streak）计算

```typescript
function findStreaks(positions: number[]): number[] {
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
```

---

## M14.2: Thumbnail Generator

### 生成策略

| 文件类型 | 生成方式 | 备注 |
|----------|----------|------|
| 图片 | macOS `sips -Z {width} -s format jpeg` | 支持 PNG/JPG/GIF/WebP/AVIF/HEIC |
| 视频 | macOS `qlmanage -t -s {width} -o /tmp` | 生成第一帧缩略图 |
| PDF | macOS `qlmanage -t -s {width} -o /tmp` | 生成第一页缩略图 |
| 其他 | 返回 null，前端使用默认图标 | — |

### 缓存策略

```
~/.natives/thumbs/
├── {hash1}.jpg          # 按文件路径 SHA256 hash 命名
├── {hash2}.jpg
└── ...
```

- 缓存键：`SHA256(filePath + ':' + width)`
- 淘汰策略：LRU，总大小超过 400MB 时淘汰最旧的
- 元数据：`~/.natives/thumbs/meta.json`（{hash, filePath, width, lastAccess}）

---

## M14.3: Git Integration

### Git 状态解析

使用 `git status --porcelain -b` 输出解析：

```
## main...origin/main
 M src/file1.ts
A  src/file2.ts
 D src/file3.ts
R  src/old.ts -> src/new.ts
?? src/untracked.ts
UU src/conflict.ts
```

解析规则：
- 前 2 字符：XY 状态码
- `##`：分支信息
- `R`：重命名（`->` 分隔）
- `??`：未跟踪文件

### Git Diff 获取

使用 `git diff HEAD -- {file}` 获取单文件 diff，直接返回文本供 Monaco DiffEditor 消费。

---

## M14.4: Disk Usage Analyzer

### 实现

```typescript
async function getDiskUsage(dirPath: string): Promise<DiskUsageItem[]> {
  const entries = await fs.promises.readdir(dirPath, { withFileTypes: true });
  const results: DiskUsageItem[] = [];

  for (const entry of entries) {
    const fullPath = path.join(dirPath, entry.name);
    if (entry.isDirectory()) {
      // 使用 du -sk 获取目录大小
      const size = await getDirectorySize(fullPath);
      results.push({ name: entry.name, path: fullPath, isDir: true, size, sizeFormatted: formatSize(size) });
    } else {
      // 使用 stat 获取文件大小
      const stat = await fs.promises.stat(fullPath);
      results.push({ name: entry.name, path: fullPath, isDir: false, size: stat.size, sizeFormatted: formatSize(stat.size) });
    }
  }

  return results.sort((a, b) => b.size - a.size); // 按大小降序
}

async function getDirectorySize(dirPath: string): Promise<number> {
  const { stdout } = await execAsync(`du -sk "${dirPath}"`);
  return parseInt(stdout.split('\t')[0]) * 1024; // KB → bytes
}
```

---

## M15: Agent Integration

### 数据结构

```typescript
// Agent 项目
interface AgentProject {
  path: string;           // 项目路径
  name: string;           // 项目名称
  engine: 'claude' | 'codex';
  lastActive: number;     // 最后活跃时间（Unix ms）
  sessionCount: number;   // 会话数量
}

// 会话信息
interface AgentSession {
  id: string;             // 会话 ID
  engine: 'claude' | 'codex';
  projectPath: string;
  title: string;          // 首条用户消息
  startTime: number;
  endTime?: number;
  filesModified: string[];// 修改的文件列表
  skillsUsed: string[];   // 使用的 Skills
}

// Skill 信息
interface SkillInfo {
  name: string;
  description: string;
  source: SkillSource;    // 来源目录
  path: string;
  enabled: boolean;
  health: SkillHealth;
  triggerCount: number;   // 45 天触发次数
  lastTriggered?: number;
}

type SkillSource =
  | '~/.claude/skills'
  | 'project/.claude/skills'
  | 'claude-plugins'
  | '~/.codex/skills'
  | '~/.agents/skills';

interface SkillHealth {
  ok: boolean;
  issues: SkillIssue[];
}

type SkillIssue =
  | 'description-truncated'   // 描述被截断（>1536 字符）
  | 'missing-frontmatter'
  | 'missing-skill-md'
  | 'residue-files';

// Claude Code 用量
interface ClaudeUsage {
  fiveHourWindow: {
    used: number;
    limit: number;
    resetAt: number;
  };
  weeklyQuota: {
    used: number;
    limit: number;
    resetAt: number;
  };
  localTokens: {
    last5h: number;
    today: number;
    thisWeek: number;
  };
}

// Codex 用量
interface CodexUsage {
  fiveHourWindow: {
    used: number;
    limit: number;
    resetAt: number;
  };
  planType: string;
}

// RTK 用量
interface RtkUsage {
  totalSaved: number;     // 总共节省的 Token 数
  totalCommands: number;  // 总命令数
  history: RtkCommandHistory[];
  topCommands: RtkCommandStat[];
}

interface RtkCommandHistory {
  command: string;
  timestamp: number;
  tokensSaved: number;
}

interface RtkCommandStat {
  command: string;
  count: number;
  totalSaved: number;
}

// 文件变更事件
interface FileChangeEvent {
  path: string;
  type: 'create' | 'modify' | 'delete';
  timestamp: number;
  sessionId?: string;     // 关联的终端会话
}

// Agent 状态
type AgentStatus = 'running' | 'idle' | 'exited';
```

### API 签名

```typescript
// === 会话管理 ===

/**
 * 扫描 Agent 项目列表
 * @param days 扫描最近 N 天（默认 30）
 * @returns 项目列表
 */
function scanAgentProjects(days?: number): Promise<AgentProject[]>;

/**
 * 获取项目的历史会话
 * @param projectPath 项目路径
 * @returns 会话列表（按时间倒序）
 */
function getProjectSessions(projectPath: string): Promise<AgentSession[]>;

/**
 * 恢复会话
 * @param sessionId 会话 ID
 * @param engine 'claude' | 'codex'
 * @returns 终端命令（如 `claude --resume {sessionId}`）
 */
function getResumeCommand(sessionId: string, engine: 'claude' | 'codex'): string;

// === Skills 管理 ===

/**
 * 扫描所有 Skills
 * @returns Skill 列表
 */
function scanSkills(): Promise<SkillInfo[]>;

/**
 * 获取 Skill 触发统计
 * @param skillName Skill 名称
 * @param days 统计最近 N 天（默认 45）
 * @returns 触发次数
 */
function getSkillTriggerCount(skillName: string, days?: number): Promise<number>;

/**
 * 启用/禁用 Skill
 * @param skillPath Skill 路径
 * @param enabled 是否启用
 */
function toggleSkill(skillPath: string, enabled: boolean): Promise<void>;

/**
 * 卸载 Skill（移到回收站）
 * @param skillPath Skill 路径
 */
function trashSkill(skillPath: string): Promise<void>;

// === 用量追踪 ===

/**
 * 获取 Claude Code 用量
 * @returns 用量信息
 */
function getClaudeUsage(): Promise<ClaudeUsage>;

/**
 * 获取 Codex 用量
 * @returns 用量信息
 */
function getCodexUsage(): Promise<CodexUsage>;

/**
 * 获取 RTK 用量
 * @returns 用量信息
 */
function getRtkUsage(): Promise<RtkUsage>;

// === 文件监控 ===

/**
 * 启动文件变更监控
 * @param projectPath 项目路径
 * @param callback 变更回调
 * @returns 停止监控的函数
 */
function startFileWatcher(projectPath: string, callback: (event: FileChangeEvent) => void): () => void;

/**
 * 检测 Agent 状态
 * @param sessionId 终端会话 ID
 * @returns Agent 状态
 */
function detectAgentStatus(sessionId: string): Promise<AgentStatus>;
```

### 实现要点

1. **会话日志路径**：
   - Claude Code: `~/.claude/projects/{encoded-path}/`
   - Codex: `~/.codex/sessions/`
2. **增量解析**：使用 `(size, mtime)` 缓存，仅解析变化的文件
3. **Skills 来源**：5 个目录扫描，跨源去重（同名 skill 在多个位置）
4. **触发统计**：从 Claude Code 会话日志中提取 `Skill tool_use` + `<command-name>` 标记
5. **RTK 用量**：解析 `rtk gain --history` 命令输出
6. **文件监控**：`fs.watch` + 噪声过滤（忽略 atime 更新、隐藏文件、SQLite sidecar）
7. **Claude Code 用量**：读取 `~/.claude/.credentials.json` 或 macOS Keychain 中的 OAuth Token，调用 Anthropic API

---

## M15.1: Session Scanner

### Claude Code 会话日志格式

```
~/.claude/projects/
├── {encoded-project-path}/
│   ├── {session-id-1}.jsonl
│   ├── {session-id-2}.jsonl
│   └── ...
```

每个 `.jsonl` 文件包含多行 JSON，每行是一个事件：
- `human`: 用户消息（首条用于标题）
- `assistant`: AI 回复
- `tool_use`: 工具调用（包含 Skill 使用信息）
- `tool_result`: 工具结果

### 解析策略

```typescript
async function parseSessionFile(filePath: string): Promise<AgentSession> {
  const content = await fs.promises.readFile(filePath, 'utf-8');
  const lines = content.split('\n').filter(Boolean);

  let title = '';
  const filesModified: string[] = [];
  const skillsUsed: string[] = [];

  for (const line of lines) {
    const event = JSON.parse(line);

    if (event.type === 'human' && !title) {
      title = event.message?.content?.[0]?.text?.slice(0, 100) || 'Untitled';
    }

    if (event.type === 'tool_use') {
      // 提取文件操作
      if (['Edit', 'Write', 'NotebookEdit'].includes(event.name)) {
        const filePath = event.input?.file_path;
        if (filePath) filesModified.push(filePath);
      }

      // 提取 Skill 使用
      if (event.name === 'Skill') {
        skillsUsed.push(event.input?.skill);
      }
    }
  }

  return {
    id: path.basename(filePath, '.jsonl'),
    engine: 'claude',
    projectPath: extractProjectPath(filePath),
    title,
    startTime: (await fs.promises.stat(filePath)).birthtimeMs,
    filesModified: [...new Set(filesModified)],
    skillsUsed: [...new Set(skillsUsed)],
  };
}
```

---

## M15.2: Skills Manager

### Skills 来源目录

| 来源 | 路径 | 说明 |
|------|------|------|
| 用户全局 | `~/.claude/skills/` | 用户安装的全局 Skills |
| 项目级 | `{project}/.claude/skills/` | 项目特定的 Skills |
| Claude 插件 | `~/.claude/plugins/` | Claude 插件中的 Skills |
| Codex | `~/.codex/skills/` | Codex Skills |
| Agents | `~/.agents/skills/` | 通用 Agent Skills |

### 健康检查

```typescript
function checkSkillHealth(skillDir: string): SkillHealth {
  const issues: SkillIssue[] = [];

  // 检查 SKILL.md 是否存在
  if (!fs.existsSync(path.join(skillDir, 'SKILL.md'))) {
    issues.push('missing-skill-md');
  }

  // 检查 frontmatter
  const skillMd = readSkillMd(skillDir);
  if (!skillMd.frontmatter) {
    issues.push('missing-frontmatter');
  }

  // 检查描述长度
  if (skillMd.description && skillMd.description.length > 1536) {
    issues.push('description-truncated');
  }

  // 检查残留文件
  const residueFiles = ['.DS_Store', 'Thumbs.db', '.git'].filter(f =>
    fs.existsSync(path.join(skillDir, f))
  );
  if (residueFiles.length > 0) {
    issues.push('residue-files');
  }

  return { ok: issues.length === 0, issues };
}
```

### 启用/禁用机制

```
~/.claude/skills/
├── my-skill/            # 启用的 Skill
│   └── SKILL.md
├── _disabled/           # 禁用的 Skills（对模型不可见）
│   └── old-skill/
│       └── SKILL.md
```

---

## M15.3: Usage Tracker

### Claude Code 用量获取

```typescript
async function getClaudeUsage(): Promise<ClaudeUsage> {
  // 1. 读取 OAuth Token
  const token = await readClaudeToken();

  // 2. 调用 Anthropic API（带代理检测）
  const proxy = await detectMacProxy();
  const response = await curlWithProxy('https://api.anthropic.com/v1/usage', {
    headers: { 'Authorization': `Bearer ${token}` },
    proxy,
  });

  // 3. 解析用量数据
  const data = JSON.parse(response);

  // 4. 补充本地 Token 统计
  const localTokens = await calculateLocalTokenStats();

  return {
    fiveHourWindow: data.five_hour_window,
    weeklyQuota: data.weekly_quota,
    localTokens,
  };
}

async function readClaudeToken(): Promise<string> {
  // 优先从 macOS Keychain 读取
  try {
    const { stdout } = await execAsync('security find-generic-password -s "claude-ai" -w');
    return stdout.trim();
  } catch {
    // 降级到文件
    const credPath = path.join(os.homedir(), '.claude', '.credentials.json');
    const cred = JSON.parse(await fs.promises.readFile(credPath, 'utf-8'));
    return cred.oauthToken;
  }
}

async function detectMacProxy(): Promise<string | null> {
  try {
    const { stdout } = await execAsync('scutil --proxy');
    const proxy = JSON.parse(stdout);
    if (proxy.HTTPEnable === 1) {
      return `http://${proxy.HTTPProxy}:${proxy.HTTPPort}`;
    }
  } catch {}
  return null;
}
```

### RTK 用量获取

```typescript
async function getRtkUsage(): Promise<RtkUsage> {
  const { stdout } = await execAsync('rtk gain --history --json');
  const data = JSON.parse(stdout);

  return {
    totalSaved: data.total_saved,
    totalCommands: data.total_commands,
    history: data.history.map((h: any) => ({
      command: h.command,
      timestamp: h.timestamp,
      tokensSaved: h.tokens_saved,
    })),
    topCommands: data.top_commands.map((c: any) => ({
      command: c.command,
      count: c.count,
      totalSaved: c.total_saved,
    })),
  };
}
```

---

## M15.4: File Watcher

### 噪声过滤规则

```typescript
function shouldIgnoreEvent(event: FileChangeEvent, context: WatchContext): boolean {
  // 1. 忽略隐藏文件
  if (path.basename(event.path).startsWith('.')) return true;

  // 2. 忽略 SQLite sidecar 文件
  const sidecarPatterns = ['-journal', '-shm', '-wal', '.tmp', '.lock'];
  if (sidecarPatterns.some(p => event.path.endsWith(p))) return true;

  // 3. 忽略只读访问（atime 更新）
  if (event.type === 'modify') {
    const stat = fs.statSync(event.path);
    if (stat.atimeMs > stat.mtimeMs + 1000) return true; // atime 比 mtime 新 1 秒以上
  }

  // 4. 忽略用户正在浏览的文件（3 秒抑制窗口）
  if (context.activeFilePath === event.path) {
    if (Date.now() - context.lastUserAccessTime < 3000) return true;
  }

  // 5. 忽略 node_modules 和 .git
  if (event.path.includes('/node_modules/') || event.path.includes('/.git/')) return true;

  return false;
}
```

### 优先级队列

```typescript
function getFilePriority(filePath: string): number {
  const ext = path.extname(filePath).toLowerCase();

  // HTML/MD 文件优先级最高（实时预览需要）
  if (['.html', '.htm', '.md', '.mdx'].includes(ext)) return 1;

  // 代码文件次之
  if (['.ts', '.tsx', '.js', '.jsx', '.py', '.rs', '.go'].includes(ext)) return 2;

  // 其他文件最低
  return 3;
}
```

---

## M16: Screenshot & Annotation

### 截图识别

```typescript
const SCREENSHOT_PATTERNS = [
  /^截屏/,           // 中文简体
  /^截圖/,           // 中文繁体
  /^截图/,           // 中文简体变体
  /^Screenshot/,     // macOS 默认
  /^Screen Shot/,    // macOS 旧版
  /^CleanShot/,      // CleanShot X
  /^SCR-/,           // 其他截图工具
];

function isScreenshot(filename: string): boolean {
  return SCREENSHOT_PATTERNS.some(p => p.test(filename));
}
```

### 浮动卡片操作

| 操作 | 行为 |
|------|------|
| 发送到终端 | 将截图路径插入终端输入框（作为 Agent 上下文） |
| 保存到素材 | 移动到当前目录的 `素材/` 子目录 |
| 标注 | 打开图片标注编辑器 |

### 图片标注工具

| 工具 | Canvas API 实现 |
|------|-----------------|
| 画笔 | `ctx.lineTo()` + `ctx.stroke()` |
| 箭头 | `ctx.moveTo()` + `ctx.lineTo()` + 三角形箭头 |
| 文字 | `ctx.fillText()` |
| 模糊 | `ctx.filter = 'blur(8px)'` + 局部重绘 |
| 遮挡 | `ctx.fillRect()` 黑色矩形 |

---

## M17: Release Wizard

### 项目检查清单

```typescript
interface ReleaseInspection {
  hasPackageJson: boolean;
  hasGit: boolean;
  isClean: boolean;         // git status 是否干净
  hasChangelog: boolean;
  hasGhCli: boolean;
  currentVersion: string;
  changelogHasUnreleased: boolean;
}
```

### 命令序列

```typescript
interface ReleaseCommandSequence {
  steps: ReleaseStep[];
}

interface ReleaseStep {
  name: string;           // 'build' | 'commit' | 'push' | 'release'
  command: string;        // 实际命令
  description: string;    // 人类可读描述
  enabled: boolean;       // 用户是否勾选
}
```

### 版本递增

```typescript
function bumpVersion(current: string, type: 'patch' | 'minor' | 'major'): string {
  const [major, minor, patch] = current.split('.').map(Number);
  switch (type) {
    case 'patch': return `${major}.${minor}.${patch + 1}`;
    case 'minor': return `${major}.${minor + 1}.0`;
    case 'major': return `${major + 1}.0.0`;
  }
}
```

---

## M18: Update Checker

### 检查策略

| 触发时机 | 节流 |
|----------|------|
| 应用启动 | 延迟 6 秒 |
| 定时检查 | 每 2 小时 |
| 窗口获焦 | 30 分钟节流 |

### 版本静音

```typescript
interface MutedVersion {
  version: string;
  mutedAt: number;
}

// 存储在 ~/.natives/config.json
{
  "mutedVersions": [
    { "version": "1.2.0", "mutedAt": 1718352000000 }
  ]
}
```

---

## Terminal Enhancement

### 可点击路径检测

```typescript
function detectFilePaths(text: string, currentDir: string): PathMatch[] {
  const matches: PathMatch[] = [];

  // 1. 绝对路径：/Users/xxx/file.ts
  const absPattern = /(?:\/[\w\-.]+)+\.\w+/g;
  let match;
  while ((match = absPattern.exec(text)) !== null) {
    matches.push({ path: match[0], start: match.index, end: match.index + match[0].length });
  }

  // 2. 相对路径：./src/file.ts 或 ../file.ts
  const relPattern = /(?:\.\.?\/[\w\-.]+)+\.\w+/g;
  while ((match = relPattern.exec(text)) !== null) {
    const resolved = path.resolve(currentDir, match[0]);
    matches.push({ path: resolved, start: match.index, end: match.index + match[0].length });
  }

  // 3. 行号引用：file.ts:42 或 file.ts:42:10
  const linePattern = /([\w\-.]+\.\w+):(\d+)(?::(\d+))?/g;
  while ((match = linePattern.exec(text)) !== null) {
    const resolved = path.resolve(currentDir, match[1]);
    matches.push({
      path: resolved,
      line: parseInt(match[2]),
      column: match[3] ? parseInt(match[3]) : undefined,
      start: match.index,
      end: match.index + match[0].length,
    });
  }

  return matches;
}
```

### 可点击链接检测

```typescript
function detectUrls(text: string): UrlMatch[] {
  const pattern = /https?:\/\/[^\s<>"]+/g;
  const matches: UrlMatch[] = [];
  let match;
  while ((match = pattern.exec(text)) !== null) {
    matches.push({ url: match[0], start: match.index, end: match.index + match[0].length });
  }
  return matches;
}
```

### Agent 状态检测

```typescript
function detectAgentStatus(output: string, exitCode?: number): AgentStatus {
  if (exitCode !== undefined) return 'exited';

  // 检测常见 AI CLI 提示符
  const idlePatterns = [
    />\s*$/,              // 通用命令提示符
    /\$\s*$/,             // bash 提示符
    />\s*$/,              // zsh 提示符
    /Claude Code.*ready/i,
    /codex.*ready/i,
  ];

  for (const pattern of idlePatterns) {
    if (pattern.test(output)) return 'idle';
  }

  return 'running';
}
```

### 跟随模式

```typescript
// 终端跟随文件浏览器
terminal.onCd((newDir: string) => {
  if (followMode === 'terminal-follow') {
    fileBrowser.navigateTo(newDir);
  }
});

// 文件浏览器跟随终端
fileBrowser.onNavigate((newDir: string) => {
  if (followMode === 'file-follow') {
    terminal.write(`cd "${newDir}"\n`);
  }
});
```

---

## Theme Simplification

### 删除 Editorial Index

需要从以下文件中移除 Editorial Index 相关代码：

1. `src/lib/theme-engine.ts` — 删除 `editorialIndex` 主题定义
2. `src/app/globals.css` — 删除 `[data-theme="editorial-index"]` CSS 变量
3. `src/components/shell/SettingsPage.tsx` — 删除主题选择器中的 Editorial Index 选项
4. 测试文件 — 更新快照测试

### 确保 2 套主题完整覆盖

验证 Terminal Volt 和 Warm Archive 的所有 CSS 变量都完整定义，包括：
- 基础色板（bg, bg-2, bg-3, panel, border, rule, text, text-dim, text-faint, accent）
- 终端 ANSI 配色
- 字体配置
- 圆角、阴影

---

## Security Hardening

### Host 头验证

```typescript
function validateHost(req: http.IncomingMessage): boolean {
  const host = req.headers.host;
  if (!host) return false;

  // 只允许 localhost 和 127.0.0.1
  const allowed = ['localhost', '127.0.0.1', `[::1]`];
  const hostname = host.split(':')[0];
  return allowed.includes(hostname);
}
```

### Origin 头验证

```typescript
function validateOrigin(req: http.IncomingMessage): boolean {
  // GET 请求不需要验证
  if (req.method === 'GET') return true;

  const origin = req.headers.origin;
  if (!origin) return true; // 非浏览器请求（如 curl）

  // 只允许来自 localhost 的请求
  try {
    const url = new URL(origin);
    return ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  } catch {
    return false;
  }
}
```

---

## Frontend Components

### 文件浏览器组件结构

```
components/files/
├── FileBrowser.tsx          # 主容器（网格/列表切换）
├── FileGrid.tsx             # 网格视图
├── FileList.tsx             # 列表视图
├── FileCard.tsx             # 网格卡片
├── FileRow.tsx              # 列表行
├── FileBreadcrumb.tsx       # 面包屑导航
├── FileToolbar.tsx          # 工具栏（排序、过滤、视图切换）
├── FilePreview.tsx          # 预览主容器
├── PreviewMarkdown.tsx      # Markdown 预览/编辑
├── PreviewCode.tsx          # 代码预览
├── PreviewHtml.tsx          # HTML 沙箱预览
├── PreviewMedia.tsx         # 多媒体预览
├── PreviewArchive.tsx       # 压缩包预览
├── FileSearch.tsx           # 搜索结果
├── GitPanel.tsx             # Git 状态
├── GitDiff.tsx              # Monaco DiffEditor
├── DiskUsage.tsx            # 磁盘用量
└── FileContextMenu.tsx      # 右键菜单
```

### AI 工作台组件结构

```
components/ai/
├── AgentMonitor.tsx         # 变更监控仪表盘
├── FileChangeCard.tsx       # 文件变更卡片（闪烁动画）
├── FollowMode.tsx           # 跟随模式控制
├── FollowNarration.tsx      # 跟随叙述行
├── ChangeInbox.tsx          # 变更收件箱
├── SessionReplay.tsx        # 会话回放（时间轴）
├── ProjectMemory.tsx        # 项目记忆
├── SessionCard.tsx          # 会话卡片
├── SkillsPanel.tsx          # Skills X-Ray
├── SkillCard.tsx            # Skill 卡片
├── UsagePanel.tsx           # Agent 用量
├── ClaudeUsage.tsx          # Claude Code 用量
├── CodexUsage.tsx           # Codex 用量
├── RtkPanel.tsx             # RTK 用量
├── FileOrganizer.tsx        # AI 文件整理
└── OrganizeDialog.tsx       # 整理对话框
```

### 工具组件结构

```
components/tools/
├── ReleaseWizard.tsx        # 发布向导
├── ReleaseStep.tsx          # 发布步骤
├── ScreenshotExpress.tsx    # 截图快递卡片
├── ImageAnnotator.tsx       # 图片标注编辑器
├── AnnotationCanvas.tsx     # 标注画布
└── AnnotationToolbar.tsx    # 标注工具栏
```

---

## IPC 频道规划

### 新增 IPC 频道

| 频道 | 方向 | 用途 |
|------|------|------|
| `fs:list` | Renderer → Main | 目录列表 |
| `fs:read` | Renderer → Main | 文件读取 |
| `fs:write` | Renderer → Main | 原子写入 |
| `fs:create` | Renderer → Main | 创建文件/目录 |
| `fs:rename` | Renderer → Main | 重命名 |
| `fs:trash` | Renderer → Main | 删除到回收站 |
| `fs:move` | Renderer → Main | 移动文件 |
| `fs:thumb` | Renderer → Main | 缩略图请求 |
| `fs:search` | Renderer → Main | 文件搜索 |
| `fs:grep` | Renderer → Main | 全文搜索 |
| `fs:git-status` | Renderer → Main | Git 状态 |
| `fs:git-diff` | Renderer → Main | Git diff |
| `fs:du` | Renderer → Main | 磁盘用量 |
| `fs:locate` | Renderer → Main | 终端路径定位 |
| `agent:projects` | Renderer → Main | Agent 项目列表 |
| `agent:sessions` | Renderer → Main | 项目会话列表 |
| `agent:skills` | Renderer → Main | Skills 列表 |
| `agent:skill-toggle` | Renderer → Main | 启用/禁用 Skill |
| `agent:skill-trash` | Renderer → Main | 卸载 Skill |
| `agent:usage-claude` | Renderer → Main | Claude 用量 |
| `agent:usage-codex` | Renderer → Main | Codex 用量 |
| `agent:usage-rtk` | Renderer → Main | RTK 用量 |
| `agent:file-change` | Main → Renderer | 文件变更通知 |
| `agent:status` | Main → Renderer | Agent 状态变更 |
| `screenshot:new` | Main → Renderer | 新截图通知 |
| `screenshot:save` | Renderer → Main | 保存标注图片 |
| `release:inspect` | Renderer → Main | 项目检查 |
| `release:prepare` | Renderer → Main | 准备发布 |
| `update:check` | Renderer → Main | 检查更新 |
| `update:info` | Main → Renderer | 更新信息 |
| `update:mute` | Renderer → Main | 静音版本 |

---

## 测试策略

### 单元测试

| 模块 | 测试点 |
|------|--------|
| File Manager | listDir 排序/过滤、readFile 截断、writeFileAtomic 冲突检测、createEntry/renameEntry/trashEntry/moveEntry |
| Search Engine | calculateScore 评分准确性、findSubsequence 边界情况、搜索结果排序 |
| Thumbnail | 生成策略、缓存命中/淘汰、文件类型检测 |
| Git Integration | porcelain 解析、diff 获取、非 git 目录处理 |
| Disk Usage | du 解析、排序、权限错误处理 |
| Session Scanner | JSONL 解析、增量缓存、标题提取 |
| Skills Manager | 5 来源扫描、健康检查、启用/禁用文件操作 |
| Usage Tracker | Token 解析、代理检测、RTK 输出解析 |
| File Watcher | 噪声过滤规则、优先级队列、抑制窗口 |
| Screenshot | 文件名模式匹配 |
| Release Wizard | 版本递增、CHANGELOG 解析、命令序列生成 |
| Update Checker | 版本比较、节流逻辑、静音管理 |
| Security | Host 头验证、Origin 头验证 |

### 集成测试

| 场景 | 测试点 |
|------|--------|
| 文件浏览器 → 预览 | 选择文件 → 右面板显示预览 |
| 文件搜索 → 定位 | Cmd+K 搜索 → 点击结果 → 导航到文件 |
| 终端 → 文件定位 | 终端输出路径 → 点击 → 文件浏览器跳转 |
| Agent 监控 → 变更 | Agent 写文件 → 文件卡片闪烁 → 变更收件箱更新 |
| 截图 → 标注 | 新截图 → 浮动卡片 → 标注 → 保存 |

### E2E 测试

| 流程 | 步骤 |
|------|------|
| 文件浏览 | 打开文件浏览器 → 导航到目录 → 切换视图 → 排序 → 选择文件 → 预览 |
| 文件操作 | 创建文件 → 重命名 → 编辑内容 → 保存 → 删除到回收站 |
| 文件搜索 | Cmd+K → 输入关键词 → 查看结果 → 点击跳转 → content: 全文搜索 |
| AI 监控 | 打开终端 → 运行 Agent → 观察文件卡片闪烁 → 查看变更收件箱 → 会话回放 |
