# Natives 整改执行计划（2026-06-16）

> **文档类型**: 可执行整改计划，交付给独立 agent 执行
> **审计依据**: 三方审计（功能完整性 / UI 合规 / i18n+错误处理）+ 硬检查
> **规范依据**: [`docs/standards/`](../../standards/README.md)（唯一权威约束源）
> **执行顺序**: 必须按 Round 1 → 2 → 3 顺序，因存在依赖（语义令牌先于颜色迁移；hook 先于空态落地）
> **每条任务都带 file:line 锚点 + 代码改动建议，执行前请重新 Read 确认行号未漂移**

---

## 执行约定

1. **每完成一个 Round，跑一次验证**：`npm run typecheck && npm run lint && npm run test`，全绿再进下一个 Round。
2. **i18n 改动**：zh.ts 和 en.ts **必须同时改**（R-I3），改完跑附录 A 的键对等脚本。
3. **遇规范冲突**：若某改动必然违反 MUST，先停，记录在「执行疑问」节，不要擅自破例。
4. **假数据红线**（R-F2）：任何用户可见数值必须有真实来源，拿不准就显示空态而非编值。
5. 行号基于审计时快照，**执行前先 Read 确认**。

---

# Round 1 · P0 合规阻塞项（MUST 红线）

## 任务 1.1 · 修复 session-scanner 的两个致命 bug【最高优先】

**文件**: `src/main/session-scanner.ts`
**问题严重度**: 🔴🔴 —— 不只是假数据，是**功能完全失效**

### Bug A：tool_use 匹配逻辑错误（功能失效，非仅假数据）

**现状**（第 35-61 行）：循环匹配 `event.type === 'tool_use'`。

**真相**（已用真实 JSONL 验证）：Claude Code 会话日志的**顶层 type 只有** `assistant`/`user`/`system`/`file-history-snapshot` 等，**没有 `tool_use`**。tool_use **嵌套在 `assistant.message.content[]` 数组**里，结构为：
```json
{ "type":"assistant", "timestamp":"2026-06-11T07:19:42.659Z",
  "message":{ "role":"assistant", "content":[
    {"type":"tool_use","id":"...","name":"Edit","input":{"file_path":"/..."}}
  ]}}
```

**后果**：`filesModified`/`fileTimestamps`/`skillsUsed` **永远是空数组** → SessionReplay 永远显示空，会话回放功能（US67）实际完全失效。

**修复**：重写解析循环，遍历 `assistant.message.content[]`：

```typescript
export function parseClaudeSessionFile(
  sessionId: string, projectPath: string, lines: string[],
): AgentSession {
  const filesModified = new Set<string>();
  const fileTimestamps: Record<string, number> = {};
  const skillsUsed = new Set<string>();
  const title = parseSessionTitle(lines);
  let startTime = 0;

  for (const line of lines) {
    let event: any;
    try { event = JSON.parse(line); } catch { continue; }

    // 时间戳在顶层（已验证格式 "2026-06-11T07:19:42.659Z"）
    const ts = event.timestamp ? Date.parse(event.timestamp) : 0;
    if (ts && (startTime === 0 || ts < startTime)) startTime = ts;

    // tool_use 嵌套在 assistant.message.content[]
    if (event.type === 'assistant' && Array.isArray(event.message?.content)) {
      for (const block of event.message.content) {
        if (block?.type !== 'tool_use') continue;
        if (['Edit', 'Write', 'NotebookEdit'].includes(block.name)) {
          const filePath = block.input?.file_path;
          if (filePath) {
            filesModified.add(filePath);
            if (!(filePath in fileTimestamps)) {
              fileTimestamps[filePath] = ts || 0;  // 真实时间戳，替换 eventIndex*100
            }
          }
        }
        if (block.name === 'Skill') {
          const skill = block.input?.skill;
          if (skill) skillsUsed.add(skill);
        }
      }
    }
  }

  return {
    id: sessionId, engine: 'claude', projectPath, title,
    startTime: startTime || Date.now(),  // 真实开始时间，替换裸 Date.now()
    filesModified: [...filesModified],
    fileTimestamps,
    skillsUsed: [...skillsUsed],
  };
}
```

**验收**：找一个真实 JSONL 会话（`~/.claude/projects/**/*.jsonl`），单元测试断言 `filesModified` 非空、`startTime` 为真实日期、`fileTimestamps` 值为合理时间戳（非 eventIndex*100 的等差数列）。在 `src/main/session-scanner.test.ts` 补测试用例（用真实日志片段做 fixture）。

### Bug B（已被 A 一并修复）：`eventIndex*100` 伪时间戳、`Date.now()` 伪开始时间 —— 见 A 的修复。

---

## 任务 1.2 · RtkPanel 假条目消除

**文件**: `src/components/ai/RtkPanel.tsx` 第 41-43 行

**现状**:
```typescript
const topCommands: RtkCommandStat[] = usage?.topCommands ?? [
  { command: 'rtk gain', count: 0, totalSaved: 0 },
];
```
当后端无数据时塞一条假的 "rtk gain" 条目展示（R-F2 违规）。

**修复**（改为空数组，走已有空态分支）:
```typescript
const topCommands: RtkCommandStat[] = usage?.topCommands ?? [];
```

**验收**：无 usage 数据时，列表显示空态文案（确认第 109-112 行的空态分支 `topCommands.length === 0` 会被触发），而非 "rtk gain" 条目。

---

## 任务 1.3 · SkillsPanel 假数据：补真实后端统计

**用户决策**：补真实后端数据（非隐藏字段）。

### 3a. 后端：从会话日志统计 skill 触发次数

**文件**: `electron/main.ts` 第 1016-1050 行 `agent:scanSkills` handler

**现状**: 返回的 skill 对象只含 `name/path/source/health`，**从不填 `triggerCount`/`lastTriggered`**，但 `SkillInfo` 类型（`src/types/agent.ts:56-57`）标为必填。

**修复**：扫描会话日志统计每个 skill 的 tool_use 次数。建议新增 `src/main/skill-stats.ts`:

```typescript
import { promises: fsp } from 'fs';
import * as path from 'path';
import * as os from 'os';

export interface SkillStat { count: number; lastTriggered?: number; }

/** 扫描 Claude Code 会话日志，统计每个 Skill 的触发次数 */
export async function getSkillStats(): Promise<Record<string, SkillStat>> {
  const stats: Record<string, SkillStat> = {};
  const projectsDir = path.join(os.homedir(), '.claude', 'projects');
  let files: string[] = [];
  try {
    // 递归找所有 .jsonl
    for (const dir of await fsp.readdir(projectsDir, { withFileTypes: true })) {
      if (!dir.isDirectory()) continue;
      const sub = path.join(projectsDir, dir.name);
      try {
        for (const f of await fsp.readdir(sub)) {
          if (f.endsWith('.jsonl')) files.push(path.join(sub, f));
        }
      } catch {}
    }
  } catch { return stats; }  // 无日志目录，返回空（诚实，不造假）

  const now = Date.now();
  for (const file of files) {
    // 只扫最近 45 天（PRD M15）
    try {
      const st = await fsp.stat(file);
      if (now - st.mtimeMs > 45 * 86400_000) continue;
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
        } catch {}
      }
    } catch {}
  }
  return stats;
}
```

然后在 `agent:scanSkills` handler（main.ts:1016）合并统计：
```typescript
// handler 内，return skills 之前：
const stats = await require('./main/skill-stats').getSkillStats();
for (const s of skills) {
  const st = stats[s.name];
  s.triggerCount = st?.count ?? 0;        // 真实统计，缺数据为 0（诚实）
  s.lastTriggered = st?.lastTriggered;     // undefined 时 UI 已有判空（第142行）
}
```

**注意**: `lazyLoad('skillsManager')` 模式下，新增模块需确认打包路径。若 lazyLoad 不便，把 `getSkillStats` 直接 inline 到 main.ts 的 handler 内也可（优先选 inline，避免模块解析问题）。

### 3b. 前端：移除伪造日志

**文件**: `src/components/ai/SkillsPanel.tsx` 第 145 行

**现状**: "Logs" 按钮点击时用 `new Date().toISOString()` 现造日志时间戳，伪装运行日志。

**修复**: Logs 改为显示**静态元数据**（来自真实扫描），不伪造时间戳。把第 145 行的 onClick 改为：
```typescript
onClick={() => {
  setSelectedSkill(skill);
  setShowLogs(true);
  // 真实元数据，不伪造时间戳（R-F2）
  const meta = [
    `Skill: ${skill.name}`,
    `Source: ${skill.source}`,
    `Path: ${skill.path}`,
    `Health: ${skill.health.ok ? 'OK' : skill.health.issues.join(', ') || 'unknown'}`,
    `Trigger count: ${skill.triggerCount}`,
    skill.lastTriggered
      ? `Last triggered: ${new Date(skill.lastTriggered).toLocaleString()}`
      : 'Last triggered: never',
  ];
  if (skill.description) meta.push(`Description: ${skill.description.slice(0, 120)}`);
  setSkillLogs(meta);
}}
```
（去掉所有 `new Date().toISOString()` 伪造）

**验收**: `skill.triggerCount` 为真实统计值；Logs 面板不再出现伪造的 `[2026-...Z]` 运行日志时间戳。无日志数据时显示"never"，不造假。

---

## 任务 1.4 · i18n 英文断链修复（6 个键）

**文件**: `src/i18n/en.ts`

**现状**（硬检查 + Agent C 交叉验证）：en.ts 的 `dashboard` 块缺 5 键（zh.ts 有），外加 `dashboard.recentlyOpened` 两边都缺。

### 4a. en.ts dashboard 块补 5 键（对照 `zh.ts` 第 48-52 行）

定位 en.ts 第 40-55 行的 `dashboard: {...}` 块，补齐（参照 zh.ts 值翻译）：
```typescript
dashboard: {
  title: 'Dashboard',
  installedModules: 'Installed Modules',
  systemStatus: 'System Status',
  quickActions: 'Quick Actions',
  openTerminal: 'Open Terminal',
  installModule: 'Install Module',
  openSettings: 'Open Settings',
  noModules: 'No modules installed yet',
  lastUsed: 'Last used',
  aiWorkbench: 'AI Workbench',
  browseManageFiles: 'Browse and manage local files',
  statusDatabase: 'Database',        // ← 新增
  statusConnected: 'Connected',       // ← 新增
  statusError: 'Error',               // ← 新增
  statusDataUsage: 'Data Usage',      // ← 新增
  fileBrowser: 'File Browser',        // ← 新增
  recentlyOpened: 'Recently Opened',  // ← 新增（zh 也要补，见 4b）
  unreadNotifications: 'Unread Notifications',
  enabledModules: 'Enabled Modules',
  recentActivity: 'Recent Activity',
},
```

### 4b. zh.ts dashboard 块补 recentlyOpened

定位 zh.ts 的 dashboard 块（第 40-55 行附近），补：
```typescript
recentlyOpened: '最近打开',  // ← 新增
```

**验收**: 跑附录 A 的键对等脚本，输出 `只在 zh: 0 / 只在 en: 0`。切换到英文 locale，仪表盘不再显示字面键名（`statusDatabase` 等）。

---

## 任务 1.5 · confirm() 消除（共 5 处）

**规范**: R-U6（MUST）—— 禁止 `confirm()`，用模态对话框替换。

**全部 5 处**（已 grep 确认）：
| 文件:行 | 用途 |
|---|---|
| `src/app/modules/page.tsx:70` | 卸载模块确认 |
| `src/components/shell/SettingsPage.tsx:160` | 删除 profile |
| `src/components/shell/SettingsPage.tsx:192` | 删除变量 |
| `src/components/shell/WorkshopPage.tsx:193` | 卸载模块 |
| `src/components/ai/SkillsPanel.tsx:58` | 卸载 skill |
| `src/components/files/FileBrowser.tsx:205` | 移入回收站（且文案未 i18n！）|

### 5a. 新建通用 ConfirmDialog 组件

**新文件**: `src/components/ui/ConfirmDialog.tsx`（放 `ui/` 因其无业务依赖，符合 R-E3）

```tsx
'use client';
import { useFocusTrap } from '@/hooks/useFocusTrap';  // 已存在

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  danger?: boolean;  // 红色确认按钮（删除/卸载场景）
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog({
  open, title, message, confirmLabel, cancelLabel, danger, onConfirm, onCancel,
}: ConfirmDialogProps) {
  const ref = useFocusTrap(open);  // 复用已有 hook
  if (!open) return null;
  return (
    <div
      style={{ position:'fixed', inset:0, background:'rgba(0,0,0,0.5)',
        display:'flex', alignItems:'center', justifyContent:'center', zIndex:60 }}
      onClick={onCancel}
    >
      <div
        ref={ref}
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        style={{ background:'var(--bg-2)', border:`1px solid var(--border)`,
          borderRadius:8, padding:20, maxWidth:400, width:'90vw' }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 style={{ fontSize:14, fontWeight:600, color:'var(--text)', marginBottom:8 }}>{title}</h3>
        <p style={{ fontSize:13, color:'var(--text-dim)', marginBottom:16, lineHeight:1.5 }}>{message}</p>
        <div style={{ display:'flex', justifyContent:'flex-end', gap:8 }}>
          <button className="btn-ghost" onClick={onCancel}>{cancelLabel}</button>
          <button
            className="btn"
            onClick={onConfirm}
            style={{ background: danger ? 'var(--danger)' : 'var(--accent)',
                     color: danger ? '#fff' : 'var(--accent-ink)',
                     border:'none', padding:'6px 14px', borderRadius:6, cursor:'pointer' }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
```
**注意**: 用到 `--danger` 令牌，需先完成 Round 2 任务 2.2（语义令牌）。若 Round 1 先做，临时用 `'#d9534f'` 硬编码并加 `// TODO: 改用 --danger（见 2.2）`，Round 2 回收。

### 5b. 逐处替换

每处把 `if (!confirm(...)) return;` 改为「设 state → 渲染 `<ConfirmDialog>` → onConfirm 里执行原逻辑」。以 `modules/page.tsx:70` 为例：

```tsx
// 组件顶部加 state
const [confirmTarget, setConfirmTarget] = useState<Module | null>(null);

// 原 handleUninstall 改为只设 target
const handleUninstall = (mod: Module) => setConfirmTarget(mod);

// 实际卸载逻辑提取
const doUninstall = async () => {
  if (!confirmTarget) return;
  try { /* 原 await nativesAPI.module.uninstall(...) */ }
  catch (err) { showErrorToast(err); }  // 顺便修 R-F5（见 Round 2）
  finally { setConfirmTarget(null); }
};

// JSX 里渲染对话框
<ConfirmDialog
  open={!!confirmTarget}
  title={t(locale, 'modules.uninstall')}
  message={t(locale, 'modules.confirmUninstallModule').replace('{name}', confirmTarget?.name ?? '')}
  confirmLabel={t(locale, 'modules.uninstall')}
  cancelLabel={t(locale, 'common.cancel')}
  danger
  onConfirm={doUninstall}
  onCancel={() => setConfirmTarget(null)}
/>
```

**FileBrowser.tsx:205 特殊处理**: 文案 `Move "${name}" to trash?` 是硬编码英文，**必须**走 i18n。新增键 `fileBrowser.confirmTrash: '确定将 "{name}" 移入废纸篓吗？'` / `'Move "{name}" to trash?'`（zh/en 同步）。

**验收**: `grep -rn "confirm(" src/`（排除 confirmXxx 标识符）应为空。6 处全部用模态。键盘可用 Tab/Enter 操作对话框。

---

# Round 2 · P1 系统性修复

## 任务 2.1 · 错误处理统一：建 useAsyncData hook

**规范**: R-E10（三态）/ R-F5（classifyError）/ R-E12

**根因**: 业务组件 catch 几乎全 `console.error` 吞掉。`showErrorToast`/`classifyError` 仅 ShellLayout 用 1 次。

**新文件**: `src/hooks/useAsyncData.ts`
```typescript
import { useState, useCallback, useRef } from 'react';
import { classifyError, type ClassifiedError } from '@/lib/error-classifier';

interface State<T> {
  data: T | null;
  loading: boolean;
  error: ClassifiedError | null;
}

export function useAsyncData<T>(
  fetcher: () => Promise<T>,
  deps: any[] = [],
) {
  const [state, setState] = useState<State<T>>({ data: null, loading: true, error: null });
  const mounted = useRef(true);

  const reload = useCallback(async () => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const data = await fetcher();
      if (mounted.current) setState({ data, loading: false, error: null });
    } catch (err) {
      if (mounted.current) setState({ data: null, loading: false, error: classifyError(err) });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  return { ...state, reload, setError: (e: unknown) => setState((s) => ({ ...s, error: classifyError(e), loading: false })) };
}
```

**迁移清单**（约 10 处，把各自的 `useState(loading)+try/catch+console.error` 换成 hook）:
- `src/app/modules/page.tsx:43-44` (module.list)
- `src/app/store/page.tsx:42-43` (module.list)
- `src/components/ai/SessionReplay.tsx:46-47`
- `src/components/ai/SkillsPanel.tsx:36-37,52-53,63-64`
- `src/components/ai/ProjectMemory.tsx:37-38,60,65-66`
- `src/components/ai/RtkPanel.tsx:30-31`
- `src/components/ai/UsagePanel.tsx:34-35`
- `src/components/ai/AIFileOrganizer.tsx:154-155,181-182`
- `src/components/shell/NotificationPanel.tsx:34-35,54-55,63-64`
- `src/components/shell/WorkshopPage.tsx:168-169,198-199`

**迁移后每处要补 error 分支**：`if (error) return <ErrorState .../>` 或 `showErrorToast`。对应 i18n 键走 `errors.*`。

---

## 任务 2.2 · 状态语义令牌（颜色迁移的前置）

**规范**: R-U1（无魔法值）/ R-U2（三皮肤键一致）

**核心**: 新增 `--danger`/`--warning`/`--info`/`--diff-add`/`--diff-del`/`--diff-mod`，三皮肤各填值。

**文件**: `src/lib/theme-engine.ts` 的 `THEMES`（第 24-67 行）

每套皮肤对象补 6 个键（注意 R-U2：三套必须键集合完全一致）:
```typescript
'terminal-volt': {
  // ...现有键...
  danger: '#f24b4b',
  'danger-soft': '#f24b4b15',
  warning: '#e6b800',
  info: '#4bcdf2',
  'diff-add': '#4ec9b0',
  'diff-del': '#f24b4b',
  'diff-mod': '#5b9cf5',
},
'warm-archive': {
  // ...现有键...（暖底配色调整对比度）
  danger: '#c0392b',
  'danger-soft': '#c0392b15',
  warning: '#b8860b',
  info: '#3a7ca5',
  'diff-add': '#5a8a5a',
  'diff-del': '#a04040',
  'diff-mod': '#5a6a9a',
},
'editorial': {
  // ...现有键...（粗野风：高对比纯色）
  danger: '#ff433d',
  'danger-soft': '#ff433d15',
  warning: '#000000',   // editorial 用黑白，warning 用黑
  info: '#0a0a0a',
  'diff-add': '#0a0a0a',
  'diff-del': '#ff433d',
  'diff-mod': '#5a5a5a',
},
```

**同步**：`ThemeSchema`（第 5-18 行）的 Zod 要加这 6 个键的校验（至少 danger/warning/info 必填，soft/diff 可选但建议必填）。

### 颜色迁移清单（约 20 处硬编码 → var(--danger) 等）

| 文件:行 | 当前 | 改为 |
|---|---|---|
| `src/lib/iframe-manager.ts:264` | `#4f46e5`（紫，最突兀）| `var(--accent)` |
| `src/app/page.tsx:300` | `#e06a5b22`/`#e06a5b` | `var(--danger-soft)`/`var(--danger)` |
| `src/app/page.tsx:215` | `#d9534f` | `var(--danger)` |
| `src/app/globals.css:509` | `.context-menu-item.danger{color:#e06a5b}` | `var(--danger)` |
| `src/app/modules/page.tsx:140` | `#e06a5b` | `var(--danger)` |
| `src/components/shell/SettingsPage.tsx:378,501` | `#e06a5b` | `var(--danger)` |
| `src/components/shell/WorkshopPage.tsx:681` | `#e06a5b` | `var(--danger)` |
| `src/components/onboarding/OnboardingWizard.tsx:116` | `#e06a5b` | `var(--danger)` |
| `src/components/ai/AgentDashboard.tsx:54` | create/delete/modify 色 | `var(--diff-add/del/mod)` |
| `src/components/ai/AgentDashboard.tsx:89,147` | `#e6b800`(paused) | `var(--warning)` |
| `src/components/ai/RtkPanel.tsx:61,134` | `#e6b800` | `var(--warning)` |
| `src/components/ai/ChangeInbox.tsx:95` | 三色 diff border | `var(--diff-*)` |
| `src/components/ai/PromptLibrary.tsx:143,148` | `#f24b4b` | `var(--danger)` |
| `src/components/ai/SkillsPanel.tsx:132` | `#4bcdf2`/`#f24b4b` | `var(--info)`/`var(--danger)` |
| `src/components/ai/SkillsPanel.tsx:148` | `#f24b4b` | `var(--danger)` |
| `src/components/crash/CrashMonitor.tsx:71,78,80` | `#f24b4b*` | `var(--danger)`/`var(--danger-soft)` |
| `src/components/shell/NotificationPanel.tsx:75-76,221` | `#e6b800`/`#d9534f` | `var(--warning)`/`var(--danger)` |
| `src/components/files/GitPanel.tsx:55` | M/A/D/R 色 | `var(--diff-*)`（git 语义对齐） |
| `src/components/files/FilePreview.tsx:530-535` | diff 行色 | `var(--diff-*)` |
| `src/components/shell/CommandPalette.tsx:219-221` | 类型色 | `var(--info)`/`var(--warning)`/新增 |

**豁免**（不改）：`FileCard.tsx` 语言品牌色（第三方标识）、`AnnotationEditor.tsx` 画笔色（用户数据）、`WorkshopPage.tsx:257-260`（生成的模板代码）。

---

## 任务 2.3 · EmptyState / Skeleton 落地

**规范**: R-U9（MUST）/ R-U10
**现状**: `EmptyState`/`Skeleton` 已定义但**全仓库零调用**。

**迁移清单**（把裸 `<div>` 兜底换成 `<EmptyState>`，约 10 处）:
| 文件:行 | 场景 |
|---|---|
| `src/app/store/page.tsx:164-189` | 模块列表空 |
| `src/app/modules/page.tsx:108-111` | 模块列表空 |
| `src/components/shell/Sidebar.tsx:196-199` | 侧边栏模块空 |
| `src/components/ai/PromptLibrary.tsx:129-132` | prompt 空（文案也需 i18n，见 2.4） |
| `src/components/ai/SkillsPanel.tsx:108-111` | skills 空 |
| `src/components/ai/RtkPanel.tsx:109-112` | commands 空 |
| `src/components/ai/SessionReplay.tsx:122-125,253-256` | sessions/文件空 |
| `src/components/ai/ProjectMemory.tsx:91-94` | sessions 空 |
| `src/components/shell/NotificationPanel.tsx:131-138` | 通知空 |
| `src/components/files/FileList.tsx:71-79` | 文件夹空 |

**模式**: `<EmptyState title={t(locale,'xxx.empty')} hint={t(locale,'xxx.emptyHint')} />`。配合 2.1 的 hook：`if (error) <ErrorState/>; if (loading) <Skeleton/>; if (!data?.length) <EmptyState/>; return <List/>`。

---

## 任务 2.4 · 三处组件全量 i18n

**规范**: R-I1（MUST）

**优先级最高的三处硬编码集中区**（其余零散文案随各任务修）:

1. **`src/components/onboarding/OnboardingWizard.tsx`**（~20 句，第 83-184 行）—— 新增 `onboarding.*` 命名空间（welcome/envCheck/aiConfig/installModules/allSet 等），zh/en 同步。
2. **`src/components/ai/PromptLibrary.tsx`**（~10 句，第 107-217 行）—— 新增 `aiWorkbench.promptLibrary.*`。
3. **`src/components/files/FileToolbar.tsx`**（~10 个 title，第 60-157 行）—— 走 `fileBrowser.*`（back/forward/ascending/newFile/newFolder/refresh/searchPlaceholder）。

**注意**: `notification-ui.ts:65,69,78,133,175`（Notifications/Mark all read/Reload 等）也硬编码，但它是命令式 DOM 非 React，`t()` 需传入 locale 参数。建议给这些函数加 locale 参数，或改用 React 版通知组件（工作量大，可降级为 P2）。

---

# Round 3 · P2 机械优化

## 任务 3.1 · 尺寸令牌消费（最系统性，按域分批）

**规范**: R-E4（MUST）/ R-U1

**前置**: `src/lib/design-tokens.ts` 的 `FONT_SIZE` 缺 9px 档（多处 `fontSize:9`）。先决定：补 `FONT_SIZE.xxs: 9` 或归并到 `xs:10`。建议补 `xxs:9`。

**分批迁移**（每批一个 PR，便于 review）:
- 批 1: `src/components/ai/`（AgentDashboard/PromptLibrary/SessionReplay/SkillsPanel/CrashMonitor）
- 批 2: `src/components/files/`
- 批 3: `src/components/shell/`

**规则**: inline style 的 `padding`→`SPACING.*`、`fontSize`→`FONT_SIZE.*`、`borderRadius`→`BORDER_RADIUS.*`。布局约束值（width/maxHeight/sidebarWidth）**保留**，不算违规。`INPUT_STYLE`（design-tokens.ts:84）自己的 `padding:'8px 10px'` 也要改。

---

## 任务 3.2 · 动效曲线令牌化

**规范**: R-U13（MUST）

**前置**: CSS 无法引用 TS 的 `TRANSITION`。在 `src/app/globals.css` 的 `:root` 补 CSS 变量:
```css
:root {
  --transition-fast: 0.12s cubic-bezier(0.16, 1, 0.3, 1);
  --transition-normal: 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  --transition-slow: 0.3s cubic-bezier(0.16, 1, 0.3, 1);
}
```

**迁移清单**（`ease`/`linear`/`ease-in-out` → 令牌曲线）:
| 文件:行 | 当前 | 改为 |
|---|---|---|
| `src/app/globals.css:191,202` | `0.15s ease` | `var(--transition-fast)` |
| `src/app/globals.css:336,346` | `0.15s ease` | `var(--transition-fast)` |
| `src/app/globals.css:669` | `0.28s ease !important` | `cubic-bezier(0.16,1,0.3,1)` |
| `src/app/globals.css:727-778` | 多处 `ease-in-out`/`ease-out` | 非循环的改令牌；循环呼吸动画 `ease-in-out` 可论证保留（脉冲需对称）|
| `src/lib/notification-ui.ts:23` | `opacity 0.2s ease` | `cubic-bezier(0.16,1,0.3,1)` |
| `src/components/ai/UsagePanel.tsx:144` | `width 0.3s ease` | `TRANSITION.slow` |
| `src/components/files/DiskUsage.tsx:103` | `width 0.3s ease` | `TRANSITION.slow` |
| `src/components/files/ImageLightbox.tsx:56` | `0.1s ease-out` | `TRANSITION.fast` |

---

## 任务 3.3 · 主标题消费 --font-display

**规范**: R-U5（SHOULD）

给以下 `<h1>` 加 `fontFamily: 'var(--font-display)'`，让 warm-archive 衬线感、editorial 块状感生效:
- `src/app/modules/page.tsx:93`
- `src/app/store/page.tsx:78`
- `src/components/shell/SettingsPage.tsx:224`
- `src/components/shell/WorkshopPage.tsx:345`

---

## 任务 3.4 · focus 可达性收尾

**规范**: R-U16

- `src/app/globals.css:436` `.input { outline: none }` —— 改为依赖 `:focus-visible` 提供焦点环（已有全局 `:focus-visible` 规则在 798-807 行）。审查 `.input:focus` 边框变色是否足以替代；若不足，补 `:focus-visible` 样式。
- `src/components/files/MilkdownEditor.tsx:89` 宿主 `outline:none` —— 评估键盘可达性，必要时恢复焦点环。

---

# 附录 A · i18n 键对等校验脚本

每次改 i18n 后跑这个，确保 zh/en 键结构完全一致:
```bash
cd /Users/ldh/Downloads/project/AiNative/Natives
python3 - <<'PY'
import re
def parse_keys(path):
    txt = open(path, encoding='utf-8').read()
    keys, stack = set(), []
    for line in txt.splitlines():
        if '//' in line: line = line.split('//')[0]
        m = re.match(r'^(\s*)([A-Za-z_$][\w$]*)\s*:', line)
        if not m:
            if re.match(r'^\s*[}\]]\s*,?\s*$', line):
                cur = len(re.match(r'^(\s*)', line).group(1))
                while stack and len(stack[-1][0]) >= cur: stack.pop()
            continue
        indent, key = m.group(1), m.group(2)
        while stack and len(stack[-1][0]) >= len(indent): stack.pop()
        stack.append((indent, key))
        keys.add('.'.join(k for _,k in stack))
    return keys
zh = parse_keys('src/i18n/zh.ts'); en = parse_keys('src/i18n/en.ts')
oz, oe = sorted(zh-en), sorted(en-zh)
print(f"zh:{len(zh)} en:{len(en)} | 只在zh:{len(oz)} 只在en:{len(oe)}")
for k in oz[:20]: print("  -zh", k)
for k in oe[:20]: print("  -en", k)
assert not oz and not oe, "i18n 键不对等！"
print("✓ zh/en 键完全一致")
PY
```

---

# 附录 B · 执行疑问（需用户决策时记录于此）

执行 agent 若遇到以下情况，停下记录在此并询问:
1. `getSkillStats` 的 lazyLoad 打包问题 —— 若无法按模块加载，inline 到 main.ts handler。
2. editorial 皮肤 warning 用纯黑是否合理 —— 当前建议 `#000000`，可能需设计确认。
3. `ConfirmDialog` 的 `--danger` 依赖 Round 2 —— 若 Round 1 先做，临时硬编码 + TODO。

---

# 附录 C · 每个 Round 的验收清单

**Round 1 完成标志**:
- [ ] `grep -rn "confirm(" src/`（排除标识符）为空
- [ ] 附录 A 脚本输出 `✓ zh/en 键完全一致`
- [ ] `RtkPanel` 无 usage 时不显示 "rtk gain" 假条目
- [ ] SessionReplay 用真实日志测试能显示被修改文件
- [ ] SkillsPanel triggerCount 来自真实统计，Logs 无伪造时间戳
- [ ] `npm run typecheck && npm run test` 全绿

**Round 2 完成标志**:
- [ ] 三皮肤均含 danger/warning/info/diff-* 键（附录 A 仍通过）
- [ ] 颜色迁移清单 20 处全部改完，`grep "#[0-9a-fA-F]\{6\}" src/components` 仅余豁免项
- [ ] 业务组件 catch 不再裸 console，走 useAsyncData/showErrorToast
- [ ] EmptyState 在 10 处列表落地

**Round 3 完成标志**:
- [ ] `:root` 含 `--transition-*` 变量
- [ ] 主标题消费 `--font-display`
- [ ] ai/ 域 inline padding/fontSize 全令牌化（批 1）
