'use client';

/**
 * RuntimePanel — 执行引擎设置页
 *
 * 忠实复刻 CodePilot RuntimePanel 的页面结构：
 *   1. 页面标题 + 描述
 *   2. 默认引擎选择器（3 个 EnginePickerCard）
 *   3. "新会话会用什么" 只读解释块
 *   4. Claude CLI 详情卡片（状态 + CLI 行 + settings.json 折叠编辑器）
 *   5. Codex CLI 详情卡片（状态 + app-server 行）
 *   6. Native 详情卡片（状态 + 能力/权限/上下文 三块）
 *   7. 能力矩阵表格
 *   8. 工具开关 + 自愈熔断
 *   9. 定时任务面板
 *
 * 术语对齐 CONTEXT.md：
 *   - Runtime = 执行引擎运行时（Claude CLI / Codex CLI / Native）
 *   - Native Runtime = Natives 自建引擎（无 CLI 降级方案）
 *   - Agent Loop = 执行回路（步限/doom/自愈熔断/心跳）
 *   - Context Assembler = 上下文装配器（静态摘要+MCP 按需深查）
 *   - Task Scheduler = 定时调度器
 */

import { useState, useEffect, useCallback, useMemo } from 'react';
import type { Locale } from '@/i18n';

// ═══════════════════════════════════════════════════════════════
// 类型
// ═══════════════════════════════════════════════════════════════

type RuntimeId = 'claude_cli' | 'codex_cli' | 'native';
type RuntimeState = 'selected' | 'available' | 'degraded' | 'blocked' | 'disabled';

interface RuntimeStatusInfo {
  state: RuntimeState;
  reason: string;
  impact: string;
  recovery?: string;
}

interface RuntimeMetadata {
  id: string;
  displayName: string;
  available: boolean;
}

interface ScheduledTask {
  id: string; name: string; prompt: string; scheduleType: string;
  scheduleValue: string; enabled: boolean; lastStatus: string | null;
  consecutiveErrors: number; nextRun: string;
}

// ═══════════════════════════════════════════════════════════════
// i18n — 全量双语表
// ═══════════════════════════════════════════════════════════════

const ZH = {
  pageTitle: '执行引擎',
  pageDesc: '查看当前 Agent 由谁运行、为什么是这个状态、影响是什么、怎么恢复。Providers 管资产，Models 管暴露，Runtime 管运行环境。',
  defaultEngine: '默认引擎',
  defaultEngineDesc: '选择新会话默认使用哪个 Runtime。当前正在运行的回复不受影响；后续每条新消息会按「默认 Runtime + Provider」重新解析。',
  whatNewChatUses: '新会话会用什么',
  whatNewChatDesc: '按当前默认设置，下一条新消息会解析为以下运行组合。每次发送前都会重新检查 Runtime、Provider 和模型兼容性 — 不持久绑定到某个会话。',
  runtimeLabel: 'Runtime',
  defaultProvider: '默认 Provider',
  defaultModel: '默认模型',
  notConfigured: '未配置',
  fallbackPath: '降级路径',
  cliDisabledFallback: 'CLI 已禁用 → 走 Native',
  cliUnavailableFallback: 'Claude CLI 不可用 → 自动用 Native',
  driftWarningCliDisabled: '保存的偏好是 Claude CLI，但 CLI 在「设置」里被显式关闭过，运行时实际走 Native。点上面任一卡片可一次写齐两边设置。',
  driftWarningCliMissing: '保存的偏好是 Claude CLI，但当前没有检测到 Claude CLI（可能未安装或登录失效），运行时实际走 Native。下方 Claude CLI 卡片提供安装入口；或者改选 Native 作为默认。',
  // Engine cards
  claudeCli: 'Claude CLI',
  claudeCliTag: 'Anthropic 官方 CLI',
  claudeCliPitch: '用 Anthropic 官方 CLI 跑 Agent，完整兼容 Claude Code 生态：~/.claude/settings.json、hooks、MCP server 直接可用。',
  codexCli: 'Codex CLI',
  codexCliTag: 'OpenAI Codex 应用服务',
  codexCliPitch: '通过 Codex 应用服务调用 ChatGPT 账户内置模型（gpt-5.5 等，额度走 ChatGPT 套餐），同时已配置的 Natives 服务商也能经 provider proxy 在 Codex 下使用。',
  nativeRuntime: 'Native 引擎',
  nativeTag: 'Natives 自带内核',
  nativePitch: 'Natives 直连 provider API 跑 Agent。适合多 provider、可观察、可恢复，由 Natives 自管上下文和权限，不依赖外部 CLI。',
  // Status
  installed: '已安装',
  installedV: '已安装 v',
  notInstalled: '未安装 — 选用后自动降级到 Native',
  alwaysAvailable: '随应用自带，始终可用',
  ready: '已就绪',
  notReady: '未就绪',
  installedIdle: '已安装，可用',
  spawnFailed: '应用服务启动失败',
  tooOld: '版本过旧',
  detecting: '检测中…',
  selected: '当前默认',
  available: '可用',
  degraded: '可用但有提示',
  blocked: '不可用',
  disabled: '已关闭',
  reason: '原因',
  impact: '影响',
  recovery: '怎么恢复',
  // Detail card rows
  cliStatus: 'CLI 状态',
  appServer: '应用服务',
  notInstalledShort: '未安装',
  install: '安装',
  update: '升级',
  refresh: '刷新',
  codexHome: 'Codex 目录',
  viewCodexAccount: '查看 Codex 账户 →',
  viewCodexModels: '查看 Codex 模型 →',
  // Native detail
  capabilities: '能力',
  capabilitiesDesc: '内置工具（Read / Edit / Bash 等），MCP 工具集，文件 / 终端 / 浏览器全套支持',
  shipsWithApp: '随应用更新',
  permissions: '权限',
  permissionsDesc: '默认 explore（读 + 安全命令自动；写 / 删 / 网络需确认），可切到 normal / trust / plan',
  perSession: '会话级控制',
  context: '上下文',
  contextDesc: 'Natives 管理项目工作区、会话历史、模型选择和本地状态；自动按 token 预算修剪 / 压缩',
  local: '本地存储',
  // Capability matrix
  capabilityMatrix: '能力矩阵',
  capability: '能力',
  // Tool settings
  toolSettings: '工具与保护',
  sideEffect: '副作用',
  // Self-heal
  selfHealTitle: '自愈熔断',
  maxSelfHeal: '自愈上限',
  maxSteps: '步数上限',
  circuitBreaker: '熔断器',
  circuitBreakerEnabled: '已启用',
  selfHealDesc: '工具执行连续失败超过上限即熔断，停止自愈，将错误升级给用户。',
  doomLoop: 'Doom Loop 检测',
  doomLoopDesc: '相同工具组合连续调用 3 次即判定 doom，中断 loop。',
  // Scheduled tasks
  tasksTitle: '定时任务',
  tasksDesc: '管理定时执行的 Agent 任务。调度器触发 runtime 层的 stream，不关心具体走哪个 runtime。',
  noTasks: '暂无任务',
  addTask: '新建任务',
  taskName: '任务名',
  taskPrompt: 'Prompt',
  taskSchedule: '调度',
  taskEnabled: '启用',
  taskNextRun: '下次执行',
  // Detect button
  detect: '检测',
  detectingBtn: '检测中…',
  // settings.json
  cliConfig: 'settings.json 配置',
  cliConfigDesc: '直接编辑 Claude CLI 的 settings.json（高级）',
  form: '表单',
  json: 'JSON',
  save: '保存',
  reset: '重置',
  format: '格式化',
  settingsSaved: '已保存',
  permissionsField: '权限 (permissions)',
  permissionsFieldDesc: 'CLI 的文件系统 / 网络权限配置',
  envVars: '环境变量 (env)',
  envVarsDesc: 'CLI 运行时注入的环境变量',
};

const EN: Record<string, string> = {
  pageTitle: 'Execution Engine',
  pageDesc: 'Inspect which runtime is currently in charge of the Agent — why it\'s in this state, what the impact is, and how to recover. Providers govern assets, Models govern exposure, Runtime governs environment.',
  defaultEngine: 'Default engine',
  defaultEngineDesc: 'Choose which runtime new chats use by default. Replies already streaming aren\'t interrupted; every subsequent message re-resolves the default runtime + provider on send.',
  whatNewChatUses: 'What a new chat will use',
  whatNewChatDesc: 'With the current defaults, your next new message resolves to the combination below. Runtime, provider, and model compatibility are re-checked on every send — nothing is pinned to a session.',
  runtimeLabel: 'Runtime',
  defaultProvider: 'Default provider',
  defaultModel: 'Default model',
  notConfigured: 'Not configured',
  fallbackPath: 'Fallback',
  cliDisabledFallback: 'CLI disabled → routes to Native',
  cliUnavailableFallback: 'Claude CLI unavailable → falls back to Native',
  driftWarningCliDisabled: 'Stored preference is Claude CLI but CLI was explicitly disabled in a previous setting, so runtime actually routes to Native. Click either card above to rewrite both fields together.',
  driftWarningCliMissing: 'Stored preference is Claude CLI but the CLI isn\'t currently detected (not installed or OAuth expired), so runtime actually routes to Native. Use the Install button on the Claude CLI card below — or pick Native as your default instead.',
  claudeCli: 'Claude CLI',
  claudeCliTag: 'Anthropic official CLI',
  claudeCliPitch: 'Runs the Agent through Anthropic\'s official CLI. Fully compatible with the Claude Code ecosystem — ~/.claude/settings.json, hooks, and MCP servers all work as-is.',
  codexCli: 'Codex CLI',
  codexCliTag: 'OpenAI Codex app-server',
  codexCliPitch: 'Routes through the Codex app-server for Codex Account models (gpt-5.5 etc., quota covered by your ChatGPT plan), and also serves configured Natives providers via the provider proxy.',
  nativeRuntime: 'Native Engine',
  nativeTag: 'Natives built-in',
  nativePitch: 'Natives calls provider APIs directly. Built for multi-provider, observable, recoverable runs — context and permissions stay inside Natives, no external CLI required.',
  installed: 'Installed',
  installedV: 'Installed v',
  notInstalled: 'Not installed — selecting it falls back to Native',
  alwaysAvailable: 'Bundled with the app, always available',
  ready: 'Ready',
  notReady: 'Not ready',
  installedIdle: 'Installed, starts on demand',
  spawnFailed: 'App-server failed to start',
  tooOld: 'Version too old',
  detecting: 'Detecting…',
  selected: 'Current default',
  available: 'Available',
  degraded: 'Available with warnings',
  blocked: 'Blocked',
  disabled: 'Disabled',
  reason: 'Reason',
  impact: 'Impact',
  recovery: 'Recovery',
  cliStatus: 'CLI status',
  appServer: 'App-server',
  notInstalledShort: 'Not installed',
  install: 'Install',
  update: 'Upgrade',
  refresh: 'Refresh',
  codexHome: 'Codex home',
  viewCodexAccount: 'View Codex account →',
  viewCodexModels: 'View Codex models →',
  capabilities: 'Capabilities',
  capabilitiesDesc: 'Built-in tools (Read / Edit / Bash / etc.), MCP toolsets, full file / terminal / browser stack',
  shipsWithApp: 'ships with app',
  permissions: 'Permissions',
  permissionsDesc: 'Defaults to Explore (auto for reads + safe commands; confirm before write / delete / network). Switchable to Normal / Trust / Plan.',
  perSession: 'per-session',
  context: 'Context',
  contextDesc: 'Natives owns project workspace, session history, model choice, and local state; automatic token-budget prune + compress.',
  local: 'local',
  capabilityMatrix: 'Capability Matrix',
  capability: 'Capability',
  toolSettings: 'Tools & Protection',
  sideEffect: 'Side-effect',
  selfHealTitle: 'Self-Heal & Circuit Breaker',
  maxSelfHeal: 'Max self-heal',
  maxSteps: 'Max steps',
  circuitBreaker: 'Circuit breaker',
  circuitBreakerEnabled: 'Enabled',
  selfHealDesc: 'Consecutive tool failures exceeding the limit trigger circuit break, stopping self-heal and escalating the error to the user.',
  doomLoop: 'Doom Loop Detection',
  doomLoopDesc: 'Same tool combination called 3 times in a row triggers doom detection, interrupting the loop.',
  tasksTitle: 'Scheduled Tasks',
  tasksDesc: 'Manage scheduled Agent tasks. The scheduler triggers runtime-level stream, regardless of which runtime is used.',
  noTasks: 'No tasks yet',
  addTask: 'New task',
  taskName: 'Task name',
  taskPrompt: 'Prompt',
  taskSchedule: 'Schedule',
  taskEnabled: 'Enabled',
  taskNextRun: 'Next run',
  detect: 'Detect',
  detectingBtn: 'Detecting…',
  cliConfig: 'settings.json config',
  cliConfigDesc: 'Directly edit Claude CLI\'s settings.json (advanced)',
  form: 'Form',
  json: 'JSON',
  save: 'Save',
  reset: 'Reset',
  format: 'Format',
  settingsSaved: 'Saved',
  permissionsField: 'Permissions (permissions)',
  permissionsFieldDesc: 'CLI filesystem / network permission config',
  envVars: 'Environment variables (env)',
  envVarsDesc: 'Environment variables injected at CLI runtime',
};

function tt(locale: Locale, key: string): string {
  return locale.startsWith('zh') ? (ZH as Record<string, string>)[key] ?? key : EN[key] ?? key;
}

// ═══════════════════════════════════════════════════════════════
// RuntimeStatusPill — 5 态状态胶囊
// ═══════════════════════════════════════════════════════════════

function RuntimeStatusPill({ state, locale }: { state: RuntimeState; locale: Locale }) {
  const tone: Record<RuntimeState, string> = {
    selected: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
    available: 'bg-zinc-500/10 text-[var(--text-dim)] dark:text-[var(--text-faint)]',
    degraded: 'bg-amber-500/15 text-amber-600 dark:text-amber-400',
    blocked: 'bg-red-500/15 text-red-600 dark:text-red-400',
    disabled: 'bg-zinc-500/10 text-[var(--text-dim)] dark:text-[var(--text-faint)]',
  };
  const dot: Record<RuntimeState, string> = {
    selected: 'bg-emerald-500', available: 'bg-zinc-400', degraded: 'bg-amber-500',
    blocked: 'bg-red-500', disabled: 'bg-zinc-400',
  };
  return (
    <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium ${tone[state]}`}>
      <span className={`size-1.5 rounded-full ${dot[state]}`} />
      {tt(locale, state)}
    </span>
  );
}

// ═══════════════════════════════════════════════════════════════
// RuntimeStatusExplanation — reason / impact / recovery
// ═══════════════════════════════════════════════════════════════

function RuntimeStatusExplanation({ info, locale }: { info: RuntimeStatusInfo; locale: Locale }) {
  const rows = [
    { label: tt(locale, 'reason'), value: info.reason },
    { label: tt(locale, 'impact'), value: info.impact },
    ...(info.recovery ? [{ label: tt(locale, 'recovery'), value: info.recovery }] : []),
  ];
  return (
    <div className="rounded-md bg-[var(--vibe-content-bg)] px-3.5 divide-y divide-[var(--vibe-btn-border)]">
      {rows.map((r) => (
        <div key={r.label} className="py-2.5 flex items-start justify-between gap-3">
          <span className="text-[11px] text-[var(--text-dim)] shrink-0">{r.label}</span>
          <span className="text-xs text-[var(--text)] text-right">{r.value}</span>
        </div>
      ))}
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// RuntimeCard — 外层卡片壳
// ═══════════════════════════════════════════════════════════════

function RuntimeCard({ name, state, locale, children }: {
  name: string; state: RuntimeState; locale: Locale; children: React.ReactNode;
}) {
  return (
    <div className="rounded-lg bg-[var(--vibe-btn-bg)] border border-[var(--vibe-btn-border)] p-5 flex flex-col gap-4">
      <div className="flex items-center gap-2 flex-wrap">
        <h3 className="text-sm font-semibold leading-tight text-[var(--text)]">{name}</h3>
        <RuntimeStatusPill state={state} locale={locale} />
      </div>
      {children}
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// EnginePickerCard — 大卡片引擎选择器
// ═══════════════════════════════════════════════════════════════

function EnginePickerCard({
  selected, onSelect, title, tagline, pitch, statusKind, statusText, locale, icon,
}: {
  selected: boolean; onSelect: () => void; title: string; tagline: string; pitch: string;
  statusKind: 'ok' | 'warning'; statusText: string; locale: Locale; icon: React.ReactNode;
}) {
  const handleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest('button, a, [role="button"]') !== e.currentTarget) return;
    onSelect();
  };
  return (
    <div
      role="button" tabIndex={0} onClick={handleClick}
      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelect(); } }}
      aria-pressed={selected}
      className={`relative w-full text-left rounded-lg border p-5 flex flex-col gap-2 transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--vibe-active-color)] ${
        selected ? 'border-[var(--vibe-active-color)] bg-[var(--vibe-active-bg)] ring-1 ring-[var(--vibe-active-color)]/30' : 'border-[var(--vibe-btn-border)] bg-[var(--vibe-btn-bg)] hover:bg-[var(--vibe-content-bg)]'
      }`}
    >
      <span className="absolute top-4 right-4 pointer-events-none">
        {selected ? (
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-[var(--vibe-active-color)]"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" /><polyline points="22 4 12 14.01 9 11.01" /></svg>
        ) : (
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-zinc-300 dark:text-zinc-600"><circle cx="12" cy="12" r="10" /></svg>
        )}
      </span>
      <div className="pr-8 flex items-start gap-2.5">
        <span className="shrink-0 mt-0.5">{icon}</span>
        <div className="min-w-0">
          <h4 className={`text-sm font-semibold ${selected ? 'text-[var(--vibe-active-color)]' : 'text-[var(--text)]'}`}>{title}</h4>
          <p className="text-sm text-[var(--text-dim)] mt-1.5">{tagline}</p>
        </div>
      </div>
      <p className="text-xs text-[var(--text-dim)] leading-relaxed line-clamp-2">{pitch}</p>
      <div className="flex items-center justify-between gap-2 mt-auto flex-wrap">
        <div className="flex items-center gap-1.5 text-[11px]">
          {statusKind === 'ok' ? (
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-emerald-500 shrink-0"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" /><polyline points="22 4 12 14.01 9 11.01" /></svg>
          ) : (
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-amber-500 shrink-0"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" /><line x1="12" y1="9" x2="12" y2="13" /><line x1="12" y1="17" x2="12.01" y2="17" /></svg>
          )}
          <span className={`truncate ${statusKind === 'ok' ? 'text-emerald-600 dark:text-emerald-400' : 'text-amber-600 dark:text-amber-400'}`}>{statusText}</span>
        </div>
      </div>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// InfoRow — 键值对行（CodePilot 风格的 divide-y 列表）
// ═══════════════════════════════════════════════════════════════

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="py-2.5 flex items-center justify-between gap-3">
      <span className="text-[11px] text-[var(--text-dim)] shrink-0">{label}</span>
      <div className="flex items-center gap-2 text-xs text-[var(--text-dim)]">{children}</div>
    </div>
  );
}

function InfoBlock({ children }: { children: React.ReactNode }) {
  return <div className="rounded-md bg-[var(--vibe-content-bg)] px-3.5 divide-y divide-[var(--vibe-btn-border)]">{children}</div>;
}

// ═══════════════════════════════════════════════════════════════
// Capability Matrix
// ═══════════════════════════════════════════════════════════════

const CAP_ROWS = [
  { key: 'memory', zh: 'Memory / 上下文', en: 'Memory / Context' },
  { key: 'widget', zh: 'Widget / UI', en: 'Widget / UI' },
  { key: 'tasks', zh: 'Tasks / 定时', en: 'Tasks / Scheduled' },
  { key: 'image', zh: 'Image / 图像', en: 'Image' },
  { key: 'media', zh: 'Media / 音视频', en: 'Media' },
  { key: 'dashboard', zh: 'Dashboard', en: 'Dashboard' },
  { key: 'cli', zh: 'CLI / 终端', en: 'CLI / Terminal' },
];

const CAP_MAP: Record<RuntimeId, Record<string, string>> = {
  claude_cli: { memory: '✅', widget: '✅', tasks: '✅', image: '✅', media: '⚠️', dashboard: '❌', cli: '✅' },
  codex_cli: { memory: '✅', widget: '✅', tasks: '✅', image: '❌', media: '❌', dashboard: '❌', cli: '⚠️' },
  native: { memory: '✅', widget: '✅', tasks: '✅', image: '✅', media: '✅', dashboard: '✅', cli: '✅' },
};

function CapabilityMatrix({ locale }: { locale: Locale }) {
  const isZh = locale.startsWith('zh');
  const runtimes: { id: RuntimeId; label: string }[] = [
    { id: 'claude_cli', label: tt(locale, 'claudeCli') },
    { id: 'codex_cli', label: tt(locale, 'codexCli') },
    { id: 'native', label: tt(locale, 'nativeRuntime') },
  ];
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs border-collapse">
        <thead>
          <tr className="border-b border-[var(--vibe-btn-border)]">
            <th className="py-2 px-3 text-left text-[var(--text-dim)] font-medium">{tt(locale, 'capability')}</th>
            {runtimes.map((r) => <th key={r.id} className="py-2 px-3 text-center text-[var(--text-dim)] font-medium">{r.label}</th>)}
          </tr>
        </thead>
        <tbody>
          {CAP_ROWS.map((row) => (
            <tr key={row.key} className="border-b border-zinc-100/50 dark:border-zinc-800/50">
              <td className="py-2 px-3 text-[var(--text)]">{isZh ? row.zh : row.en}</td>
              {runtimes.map((r) => <td key={r.id} className="py-2 px-3 text-center">{CAP_MAP[r.id][row.key]}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// Tool Toggle
// ═══════════════════════════════════════════════════════════════

function ToolToggle({ label, sideEffect, enabled, onToggle, locale }: {
  label: string; sideEffect: boolean; enabled: boolean; onToggle: () => void; locale: Locale;
}) {
  return (
    <div className="flex items-center justify-between py-2 px-3 rounded-lg bg-[var(--vibe-content-bg)] border border-[var(--vibe-btn-border)]">
      <div className="flex items-center gap-2">
        <span className="text-sm text-[var(--text)]">{label}</span>
        {sideEffect && <span className="text-[10px] px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-600 dark:text-amber-400">{tt(locale, 'sideEffect')}</span>}
      </div>
      <button onClick={onToggle} role="switch" aria-checked={enabled}
        className={`relative w-9 h-5 rounded-full transition-colors ${enabled ? 'bg-[var(--vibe-active-bg)]' : 'bg-zinc-300 dark:bg-zinc-600'}`}>
        <span className={`absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-all ${enabled ? 'left-[18px]' : 'left-0.5'}`} />
      </button>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// Scheduled Tasks Panel
// ═══════════════════════════════════════════════════════════════

function ScheduledTasksPanel({ locale }: { locale: Locale }) {
  const [tasks, setTasks] = useState<ScheduledTask[]>([]);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    (async () => {
      try { const r = await window.nativesAPI?.scheduler?.listTasks?.() as ScheduledTask[] | undefined; if (r) setTasks(r); } catch { /* */ }
      finally { setLoading(false); }
    })();
  }, []);
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-[var(--text-dim)]">{tt(locale, 'tasksDesc')}</p>
      {loading ? <div className="text-sm text-[var(--text-faint)] py-4 text-center">…</div>
        : tasks.length === 0 ? <div className="text-sm text-[var(--text-faint)] py-4 text-center">{tt(locale, 'noTasks')}</div>
        : <div className="flex flex-col gap-2">{tasks.map((t) => (
          <div key={t.id} className="flex items-center justify-between py-2 px-3 rounded-lg bg-[var(--vibe-content-bg)] border border-[var(--vibe-btn-border)]">
            <div className="flex flex-col gap-0.5 min-w-0">
              <span className="text-sm font-medium text-[var(--text)] truncate">{t.name}</span>
              <span className="text-xs text-[var(--text-dim)] truncate">{t.prompt}</span>
            </div>
            <div className="flex items-center gap-3 shrink-0">
              <span className={`text-[10px] px-1.5 py-0.5 rounded ${t.enabled ? 'bg-emerald-500/15 text-emerald-600' : 'bg-zinc-500/10 text-[var(--text-dim)]'}`}>{t.enabled ? tt(locale, 'taskEnabled') : 'Off'}</span>
              <span className="text-[11px] text-[var(--text-dim)]">{t.nextRun}</span>
            </div>
          </div>
        ))}</div>
      }
      <button className="self-start text-xs text-[var(--vibe-active-color)] hover:text-blue-600 font-medium">{tt(locale, 'addTask')}</button>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// SVG 图标组件
// ═══════════════════════════════════════════════════════════════

const Icons = {
  Check: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-emerald-500"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" /><polyline points="22 4 12 14.01 9 11.01" /></svg>,
  X: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-red-500"><circle cx="12" cy="12" r="10" /><line x1="15" y1="9" x2="9" y2="15" /><line x1="9" y1="9" x2="15" y2="15" /></svg>,
  Warning: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-amber-500"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" /><line x1="12" y1="9" x2="12" y2="13" /><line x1="12" y1="17" x2="12.01" y2="17" /></svg>,
  Refresh: () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="23 4 23 10 17 10" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" /></svg>,
  Anthropic: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2z" fill="#D97706" opacity=".15" /><text x="12" y="16" textAnchor="middle" fontSize="10" fill="#D97706" fontWeight="bold">A</text></svg>,
  OpenAI: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="10" stroke="#10B981" strokeWidth="1.5" /><path d="M8 12l3 3 5-6" stroke="#10B981" strokeWidth="2" /></svg>,
  Native: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none"><rect x="3" y="3" width="18" height="18" rx="3" stroke="#3B82F6" strokeWidth="1.5" /><circle cx="12" cy="12" r="4" fill="#3B82F6" opacity=".2" /></svg>,
  Code: () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="16 18 22 12 16 6" /><polyline points="8 6 2 12 8 18" /></svg>,
  Chevron: () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="6 9 12 15 18 9" /></svg>,
};

// ═══════════════════════════════════════════════════════════════
// Main RuntimePanel
// ═══════════════════════════════════════════════════════════════

export default function RuntimePanel({ locale }: { locale: Locale }) {
  const isZh = locale.startsWith('zh');

  // ── Runtime state ──
  const [selectedRuntime, setSelectedRuntime] = useState<RuntimeId>('native');
  const [detecting, setDetecting] = useState(false);
  const [claudeAvailable, setClaudeAvailable] = useState(false);
  const [codexAvailable, setCodexAvailable] = useState(false);
  const [claudeVersion, setClaudeVersion] = useState<string | null>(null);
  const [codexVersion, setCodexVersion] = useState<string | null>(null);

  // ── Tool settings ──
  const [enabledTools, setEnabledTools] = useState<Record<string, boolean>>({
    read_file: true, list_dir: true, write_file: true,
    write_module: true, run_terminal: false, lint_module: true,
  });
  const [maxSelfHeal, setMaxSelfHeal] = useState(3);
  const [maxSteps, setMaxSteps] = useState(50);
  const [executorLoaded, setExecutorLoaded] = useState(false);

  // ── Load data ──
  useEffect(() => {
    (async () => {
      try {
        const [rtResult, execResult] = await Promise.all([
          window.nativesAPI?.runtime?.listAvailable?.() as RuntimeMetadata[] | undefined,
          window.nativesAPI?.executorSettings?.get?.() as { enabledTools: Record<string, boolean>; maxSelfHeal: number; maxSteps?: number } | undefined,
        ]);
        if (rtResult) {
          setClaudeAvailable(rtResult.some(r => r.id === 'claude_cli' && r.available));
          setCodexAvailable(rtResult.some(r => r.id === 'codex_cli' && r.available));
        }
        if (execResult) {
          setEnabledTools(execResult.enabledTools ?? enabledTools);
          setMaxSelfHeal(execResult.maxSelfHeal ?? 3);
          setMaxSteps(execResult.maxSteps ?? 50);
          setExecutorLoaded(true);
        }
      } catch (e) { console.error('Failed to load runtime settings:', e); }
    })();
  }, []);

  // ── Save executor ──
  const saveExecutor = useCallback(async (next: Record<string, boolean>, selfHeal: number, steps: number) => {
    if (!executorLoaded) return;
    try { await window.nativesAPI?.executorSettings?.save?.({ enabledTools: next, maxSelfHeal: selfHeal, maxSteps: steps }); } catch (e) { console.error('Failed to save:', e); }
  }, [executorLoaded]);

  const toggleTool = useCallback((key: string) => {
    setEnabledTools(prev => { const next = { ...prev, [key]: !prev[key] }; saveExecutor(next, maxSelfHeal, maxSteps); return next; });
  }, [maxSelfHeal, maxSteps, saveExecutor]);

  const updateMaxSelfHeal = useCallback((val: number) => {
    const c = Math.max(1, Math.min(10, val)); setMaxSelfHeal(c); saveExecutor(enabledTools, c, maxSteps);
  }, [enabledTools, maxSteps, saveExecutor]);

  const updateMaxSteps = useCallback((val: number) => {
    const c = Math.max(10, Math.min(200, val)); setMaxSteps(c); saveExecutor(enabledTools, maxSelfHeal, c);
  }, [enabledTools, maxSelfHeal, saveExecutor]);

  // ── Detect CLI ──
  const handleDetect = useCallback(async () => {
    setDetecting(true);
    try {
      const result = await window.nativesAPI?.runtime?.detectCli?.() as { claude_cli: boolean; codex_cli: boolean } | undefined;
      if (result) { setClaudeAvailable(result.claude_cli); setCodexAvailable(result.codex_cli); }
    } catch { /* */ }
    finally { setDetecting(false); }
  }, []);

  // ── Runtime status computation ──
  const getRuntimeStatus = useMemo(() => (id: RuntimeId): RuntimeStatusInfo => {
    const isSelected = selectedRuntime === id;
    if (id === 'claude_cli') {
      if (!claudeAvailable) return { state: 'blocked', reason: isZh ? '未检测到 Claude CLI（或 OAuth 登录已过期）' : 'Claude CLI not detected (or OAuth login has expired)', impact: isZh ? '无法用 Claude CLI 跑会话；选用后自动降级到 Native' : 'Sessions cannot run on Claude CLI; selecting it falls back to Native', recovery: isZh ? '安装 Claude CLI 后点「检测」刷新，或在系统终端 `claude /login` 完成授权' : 'Install Claude CLI then click Detect, or run `claude /login` in a terminal' };
      return isSelected
        ? { state: 'selected', reason: isZh ? 'Claude CLI 已安装并被设为默认引擎' : 'Claude CLI is installed and set as the default engine', impact: isZh ? '新会话默认走 Claude CLI 内核，使用 ~/.claude/settings.json 中的环境与权限' : 'New chats run on the Claude CLI engine, honoring ~/.claude/settings.json' }
        : { state: 'available', reason: isZh ? 'Claude CLI 已安装但未被设为默认引擎' : 'Claude CLI is installed but isn\'t the default engine', impact: isZh ? '想切回 Claude CLI 内核，把上方「默认引擎」切到 Claude CLI 即可' : 'Switch the "Default engine" selector above to use Claude CLI' };
    }
    if (id === 'codex_cli') {
      if (!codexAvailable) return { state: 'blocked', reason: isZh ? '未在 PATH 上检测到 codex 命令' : 'codex binary not detected on PATH', impact: isZh ? 'Codex Runtime 整体无法启用' : 'Codex Runtime is fully blocked', recovery: isZh ? '按 Codex 官方指引安装 codex CLI，或设置 CODEX_BIN 指向自定义路径' : 'Install codex CLI per the official guide, or set CODEX_BIN to point at a custom binary' };
      return isSelected
        ? { state: 'selected', reason: isZh ? 'Codex 应用服务已就绪并被设为默认引擎' : 'Codex app-server is ready and set as the default engine', impact: isZh ? '新会话默认走 Codex' : 'New chats run on Codex' }
        : { state: 'available', reason: isZh ? 'Codex 应用服务已就绪但未被设为默认' : 'Codex app-server is ready but not the default engine', impact: isZh ? '想把 Codex 设为默认，把上方「默认引擎」切到 Codex' : 'Switch the "Default engine" selector above to make Codex the default' };
    }
    return isSelected
      ? { state: 'selected', reason: isZh ? 'Native 是默认内核（无需 CLI，直连 provider API）' : 'Native is the default engine (no CLI required, direct provider API)', impact: isZh ? '新会话默认用 Native；工具、权限和上下文由 Natives 自己管理' : 'New chats run on Native; tools, permissions, and context managed by Natives itself' }
      : { state: 'available', reason: isZh ? 'Native 内核随应用自带，始终可用' : 'Native ships with the app and is always available', impact: isZh ? '想切到 Native 内核，把上方「默认引擎」切到 Native 即可' : 'Switch the "Default engine" selector above to use Native' };
  }, [selectedRuntime, claudeAvailable, codexAvailable, isZh]);

  const driftWarning = selectedRuntime === 'claude_cli' && !claudeAvailable;

  // ── Tool definitions ──
  const tools = [
    { key: 'read_file', label: isZh ? '读取文件' : 'Read File', sideEffect: false },
    { key: 'list_dir', label: isZh ? '列出目录' : 'List Dir', sideEffect: false },
    { key: 'write_file', label: isZh ? '写入文件' : 'Write File', sideEffect: true },
    { key: 'write_module', label: isZh ? '写入模块' : 'Write Module', sideEffect: true },
    { key: 'run_terminal', label: isZh ? '运行终端' : 'Run Terminal', sideEffect: true },
    { key: 'lint_module', label: isZh ? '检查模块' : 'Lint Module', sideEffect: false },
  ];

  const sectionCard = 'rounded-lg bg-[var(--vibe-btn-bg)] border border-[var(--vibe-btn-border)] p-5 flex flex-col gap-4';
  const sectionTitle = 'text-sm font-semibold text-[var(--text)] uppercase tracking-wider';

  return (
    <div className="max-w-4xl mx-auto space-y-8">
      {/* ── 1. 页面标题 ── */}
      <div>
        <h2 className="text-xl font-semibold tracking-tight text-[var(--text)]">{tt(locale, 'pageTitle')}</h2>
        <p className="text-sm text-[var(--text-dim)] mt-1.5">{tt(locale, 'pageDesc')}</p>
      </div>

      {/* ── 2. 默认引擎选择器 ── */}
      <div>
        <div className="flex items-center justify-between mb-2">
          <h3 className="text-sm font-semibold text-[var(--text)]">{tt(locale, 'defaultEngine')}</h3>
          <button onClick={handleDetect} disabled={detecting} className="text-xs text-[var(--vibe-active-color)] hover:text-blue-600 font-medium disabled:opacity-50">
            {detecting ? tt(locale, 'detectingBtn') : tt(locale, 'detect')}
          </button>
        </div>
        <p className="text-[11px] text-[var(--text-dim)] mb-3">{tt(locale, 'defaultEngineDesc')}</p>

        {driftWarning && (
          <div className="mb-3 rounded-md border border-amber-500/20 bg-amber-500/5 px-3 py-2 text-[11px] text-amber-600 dark:text-amber-400 flex items-start gap-1.5">
            <Icons.Warning />
            <span>{tt(locale, 'driftWarningCliMissing')}</span>
          </div>
        )}

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <EnginePickerCard
            selected={selectedRuntime === 'claude_cli'} onSelect={() => setSelectedRuntime('claude_cli')}
            title={tt(locale, 'claudeCli')} icon={<Icons.Anthropic />} tagline={tt(locale, 'claudeCliTag')} pitch={tt(locale, 'claudeCliPitch')}
            statusKind={claudeAvailable ? 'ok' : 'warning'} statusText={claudeAvailable ? `${tt(locale, 'installedV')}${claudeVersion ?? ''}` : tt(locale, 'notInstalled')} locale={locale}
          />
          <EnginePickerCard
            selected={selectedRuntime === 'codex_cli'} onSelect={() => setSelectedRuntime('codex_cli')}
            title={tt(locale, 'codexCli')} icon={<Icons.OpenAI />} tagline={tt(locale, 'codexCliTag')} pitch={tt(locale, 'codexCliPitch')}
            statusKind={codexAvailable ? 'ok' : 'warning'} statusText={codexAvailable ? tt(locale, 'ready') : tt(locale, 'notReady')} locale={locale}
          />
          <EnginePickerCard
            selected={selectedRuntime === 'native'} onSelect={() => setSelectedRuntime('native')}
            title={tt(locale, 'nativeRuntime')} icon={<Icons.Native />} tagline={tt(locale, 'nativeTag')} pitch={tt(locale, 'nativePitch')}
            statusKind="ok" statusText={tt(locale, 'alwaysAvailable')} locale={locale}
          />
        </div>
      </div>

      {/* ── 3. "新会话会用什么" 只读解释块 ── */}
      <div className={sectionCard}>
        <h3 className="text-sm font-semibold leading-tight text-[var(--text)]">{tt(locale, 'whatNewChatUses')}</h3>
        <p className="text-[11px] text-[var(--text-dim)]">{tt(locale, 'whatNewChatDesc')}</p>
        <InfoBlock>
          <InfoRow label={tt(locale, 'runtimeLabel')}>{selectedRuntime === 'claude_cli' ? tt(locale, 'claudeCli') : selectedRuntime === 'codex_cli' ? tt(locale, 'codexCli') : tt(locale, 'nativeRuntime')}</InfoRow>
          <InfoRow label={tt(locale, 'defaultProvider')}>{tt(locale, 'notConfigured')}</InfoRow>
          <InfoRow label={tt(locale, 'defaultModel')}>{tt(locale, 'notConfigured')}</InfoRow>
          {driftWarning && <InfoRow label={tt(locale, 'fallbackPath')}><span className="text-amber-600 dark:text-amber-400">{tt(locale, 'cliUnavailableFallback')}</span></InfoRow>}
        </InfoBlock>
      </div>

      {/* ── 4. Claude CLI 详情卡片 ── */}
      <RuntimeCard name={tt(locale, 'claudeCli')} state={getRuntimeStatus('claude_cli').state} locale={locale}>
        <RuntimeStatusExplanation info={getRuntimeStatus('claude_cli')} locale={locale} />
        <InfoBlock>
          <InfoRow label={tt(locale, 'cliStatus')}>
            {claudeAvailable ? (<><Icons.Check /><span className="font-mono">v{claudeVersion ?? '?'}</span></>) : (<><Icons.X /><span>{tt(locale, 'notInstalledShort')}</span></>)}
            <button onClick={handleDetect} className="p-1 rounded hover:bg-[var(--vibe-content-bg)]"><Icons.Refresh /></button>
          </InfoRow>
        </InfoBlock>
        {/* settings.json 折叠编辑器（CodePilot 风格） */}
        <details className="rounded-md bg-[var(--vibe-content-bg)] px-3.5 py-2 group">
          <summary className="flex items-center justify-between gap-2 cursor-pointer text-xs font-medium select-none list-none text-[var(--text-dim)]">
            <span className="flex items-center gap-1.5"><Icons.Code />{tt(locale, 'cliConfig')}</span>
            <span className="transition-transform group-open:rotate-180"><Icons.Chevron /></span>
          </summary>
          <p className="mt-1 mb-3 text-[11px] text-[var(--text-dim)]">{tt(locale, 'cliConfigDesc')}</p>
          <div className="space-y-3">
            <div>
              <label className="text-xs font-medium text-[var(--text)]">{tt(locale, 'permissionsField')}</label>
              <p className="mb-1.5 text-[11px] text-[var(--text-dim)]">{tt(locale, 'permissionsFieldDesc')}</p>
              <textarea className="w-full font-mono text-xs rounded-md border border-[var(--vibe-btn-border)] bg-white dark:bg-zinc-800 p-2 outline-none focus:ring-1 focus:ring-[var(--vibe-active-color)]" rows={3} placeholder='{"allow": ["Read", "Write"]}' />
            </div>
            <div>
              <label className="text-xs font-medium text-[var(--text)]">{tt(locale, 'envVars')}</label>
              <p className="mb-1.5 text-[11px] text-[var(--text-dim)]">{tt(locale, 'envVarsDesc')}</p>
              <textarea className="w-full font-mono text-xs rounded-md border border-[var(--vibe-btn-border)] bg-white dark:bg-zinc-800 p-2 outline-none focus:ring-1 focus:ring-[var(--vibe-active-color)]" rows={3} placeholder='{"KEY": "value"}' />
            </div>
          </div>
        </details>
      </RuntimeCard>

      {/* ── 5. Codex CLI 详情卡片 ── */}
      <RuntimeCard name={tt(locale, 'codexCli')} state={getRuntimeStatus('codex_cli').state} locale={locale}>
        <RuntimeStatusExplanation info={getRuntimeStatus('codex_cli')} locale={locale} />
        <InfoBlock>
          <InfoRow label={tt(locale, 'appServer')}>
            {codexAvailable ? (<><Icons.Check /><span className="font-mono">{codexVersion ?? ''}</span></>) : (<><Icons.X /><span>{tt(locale, 'notInstalledShort')}</span></>)}
            <button onClick={handleDetect} className="p-1 rounded hover:bg-[var(--vibe-content-bg)]"><Icons.Refresh /></button>
          </InfoRow>
        </InfoBlock>
        <div className="flex flex-wrap gap-2 justify-end">
          <button className="text-xs text-[var(--vibe-active-color)] hover:text-blue-600 font-medium">{tt(locale, 'viewCodexAccount')}</button>
          <button className="text-xs text-[var(--vibe-active-color)] hover:text-blue-600 font-medium">{tt(locale, 'viewCodexModels')}</button>
        </div>
      </RuntimeCard>

      {/* ── 6. Native 详情卡片 ── */}
      <RuntimeCard name={tt(locale, 'nativeRuntime')} state={getRuntimeStatus('native').state} locale={locale}>
        <RuntimeStatusExplanation info={getRuntimeStatus('native')} locale={locale} />
        <InfoBlock>
          <div className="py-2.5 flex items-start justify-between gap-3">
            <div className="flex flex-col gap-0.5 max-w-[55%]">
              <span className="text-xs font-medium text-[var(--text)]">{tt(locale, 'capabilities')}</span>
              <span className="text-[11px] text-[var(--text-dim)] leading-snug">{tt(locale, 'capabilitiesDesc')}</span>
            </div>
            <span className="text-[10px] text-[var(--text-faint)]">{tt(locale, 'shipsWithApp')}</span>
          </div>
          <div className="py-2.5 flex items-start justify-between gap-3">
            <div className="flex flex-col gap-0.5 max-w-[55%]">
              <span className="text-xs font-medium text-[var(--text)]">{tt(locale, 'permissions')}</span>
              <span className="text-[11px] text-[var(--text-dim)] leading-snug">{tt(locale, 'permissionsDesc')}</span>
            </div>
            <span className="text-[10px] text-[var(--text-faint)]">{tt(locale, 'perSession')}</span>
          </div>
          <div className="py-2.5 flex items-start justify-between gap-3">
            <div className="flex flex-col gap-0.5 max-w-[55%]">
              <span className="text-xs font-medium text-[var(--text)]">{tt(locale, 'context')}</span>
              <span className="text-[11px] text-[var(--text-dim)] leading-snug">{tt(locale, 'contextDesc')}</span>
            </div>
            <span className="text-[10px] text-[var(--text-faint)]">{tt(locale, 'local')}</span>
          </div>
        </InfoBlock>
      </RuntimeCard>

      {/* ── 7. 能力矩阵 ── */}
      <div className={sectionCard}>
        <h3 className={sectionTitle}>{tt(locale, 'capabilityMatrix')}</h3>
        <CapabilityMatrix locale={locale} />
      </div>

      {/* ── 8. 工具开关 + 自愈熔断 ── */}
      <div className={sectionCard}>
        <h3 className={sectionTitle}>{tt(locale, 'toolSettings')}</h3>
        <div className="flex flex-col gap-2">
          {tools.map(tool => <ToolToggle key={tool.key} label={tool.label} sideEffect={tool.sideEffect} enabled={enabledTools[tool.key] ?? false} onToggle={() => toggleTool(tool.key)} locale={locale} />)}
        </div>
      </div>

      <div className={sectionCard}>
        <h3 className={sectionTitle}>{tt(locale, 'selfHealTitle')}</h3>
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <span className="text-sm text-[var(--text)]">{tt(locale, 'maxSelfHeal')}</span>
            <input type="number" min={1} max={10} value={maxSelfHeal} onChange={(e) => updateMaxSelfHeal(Number(e.target.value))}
              className="w-14 text-sm font-semibold text-[var(--vibe-active-color)] text-center py-1 px-2 rounded-md bg-zinc-50 dark:bg-zinc-800 border border-[var(--vibe-btn-border)] outline-none" />
          </div>
          <p className="text-xs text-[var(--text-dim)]">{tt(locale, 'selfHealDesc')}</p>
          <div className="flex items-center justify-between">
            <span className="text-sm text-[var(--text)]">{tt(locale, 'maxSteps')}</span>
            <input type="number" min={10} max={200} value={maxSteps} onChange={(e) => updateMaxSteps(Number(e.target.value))}
              className="w-14 text-sm font-semibold text-[var(--vibe-active-color)] text-center py-1 px-2 rounded-md bg-zinc-50 dark:bg-zinc-800 border border-[var(--vibe-btn-border)] outline-none" />
          </div>
          <div className="flex items-center justify-between">
            <span className="text-sm text-[var(--text)]">{tt(locale, 'circuitBreaker')}</span>
            <span className="text-xs text-red-500 font-medium">{tt(locale, 'circuitBreakerEnabled')}</span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-sm text-[var(--text)]">{tt(locale, 'doomLoop')}</span>
            <span className="text-xs text-[var(--text-dim)]">3 {isZh ? '次' : 'times'}</span>
          </div>
          <p className="text-xs text-[var(--text-dim)]">{tt(locale, 'doomLoopDesc')}</p>
        </div>
      </div>

      {/* ── 9. 定时任务 ── */}
      <div className={sectionCard}>
        <h3 className={sectionTitle}>{tt(locale, 'tasksTitle')}</h3>
        <ScheduledTasksPanel locale={locale} />
      </div>
    </div>
  );
}
