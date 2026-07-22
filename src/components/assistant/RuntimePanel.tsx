'use client';

/**
 * RuntimePanel — 执行引擎设置页
 *
 * 忠实复刻 CodePilot RuntimePanel 的页面结构：
 *   1. 页面标题 + 描述
 *   2. 默认引擎选择器（3 个 EnginePickerCard）
 *   3. "新会话会用什么" 只读解释块
 *   4. Claude CLI 详情卡片（状态 + CLI 行 + 安全边界说明）
 *   5. Codex CLI 详情卡片（状态 + app-server 行）
 *   6. Native 详情卡片（状态 + 能力/权限/上下文 三块）
 *   7. 能力矩阵表格
 *   8. 工具开关 + 自愈熔断
 *   9. 定时任务面板
 */

import { useState, useEffect, useCallback, useMemo } from 'react';
import type { Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import {
  loadPreferredRuntimeId,
  savePreferredRuntimeId,
} from '@/lib/assistant-workspace/persistence';
import { useToast } from '@/components/ui/Toast';

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

const DEFAULT_ENABLED_TOOLS: Record<string, boolean> = {
  read_file: true,
  list_dir: true,
  write_file: true,
  write_module: true,
  run_terminal: false,
  lint_module: true,
};

// ═══════════════════════════════════════════════════════════════
// i18n — 全量双语表
// ═══════════════════════════════════════════════════════════════

const ZH = {
  pageTitle: '执行引擎',
  pageDesc: '查看当前 Agent 由谁运行、为什么是这个状态、影响是什么、怎么恢复。Providers 管资产，Models 管暴露，Runtime 管运行环境。',
  defaultEngine: '默认引擎',
  defaultEngineDesc: '选择新会话默认使用哪个 Runtime。当前正在运行的回复不受影响；后续每条新消息会按「默认 Runtime + Provider」重新解析。',
  whatNewChatUses: '新会话会用什么',
  whatNewChatDesc: '按当前默认设置，下一条新消息会解析为以下运行组合。每次发送前都会重新检查 Runtime、Provider 和模型兼容性 - 不持久绑定到某个会话。',
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
  notInstalled: '未安装 - 选用后自动降级到 Native',
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
  codexInspectionUnavailable: 'Codex 账户与模型读取尚未接入真实 app-server API；当前版本只展示 CLI 可用性，避免显示不可信账户信息。',
  loadRuntimeFailed: '执行引擎设置加载失败',
  saveRuntimeFailed: '执行引擎设置保存失败',
  detectRuntimeFailed: 'CLI 检测失败',
  tasksLoadFailed: '定时任务加载失败',
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
  taskName: '任务名',
  taskPrompt: 'Prompt',
  taskSchedule: '调度',
  taskEnabled: '启用',
  taskNextRun: '下次执行',
  // Detect button
  detect: '重新检测环境',
  detectingBtn: '检测中…',
  // Claude CLI safety
  cliConfig: 'Claude CLI 配置边界',
  cliConfigDesc: 'AiNative 会读取 Claude CLI 是否可用，并在运行时通过受控子进程调用它；当前版本不会编辑 ~/.claude/settings.json，也不会修改你的 shell、终端或全局 Claude 配置。',
  cliConfigUserOwned: '用户自主管理',
  cliConfigUserOwnedDesc: '权限、hooks、MCP server 和 OAuth 登录仍由 Claude CLI 自己管理；需要变更时请在 Claude 官方 CLI 中完成。',
  cliConfigSandbox: 'AiNative 沙盒',
  cliConfigSandboxDesc: 'AiNative 只管理自己的 Runtime、Provider、Key 和终端沙盒配置；所有 Key 仍由后端加密管理，不写入 Claude 全局配置。',
};

const EN: Record<string, string> = {
  pageTitle: 'Execution Engine',
  pageDesc: 'Inspect which runtime is currently in charge of the Agent - why it\'s in this state, what the impact is, and how to recover. Providers govern assets, Models govern exposure, Runtime governs environment.',
  defaultEngine: 'Default engine',
  defaultEngineDesc: 'Choose which runtime new chats use by default. Replies already streaming aren\'t interrupted; every subsequent message re-resolves the default runtime + provider on send.',
  whatNewChatUses: 'What a new chat will use',
  whatNewChatDesc: 'With the current defaults, your next new message resolves to the combination below. Runtime, provider, and model compatibility are re-checked on every send - nothing is pinned to a session.',
  runtimeLabel: 'Runtime',
  defaultProvider: 'Default provider',
  defaultModel: 'Default model',
  notConfigured: 'Not configured',
  fallbackPath: 'Fallback',
  cliDisabledFallback: 'CLI disabled → routes to Native',
  cliUnavailableFallback: 'Claude CLI unavailable → falls back to Native',
  driftWarningCliDisabled: 'Stored preference is Claude CLI but CLI was explicitly disabled in a previous setting, so runtime actually routes to Native. Click either card above to rewrite both fields together.',
  driftWarningCliMissing: 'Stored preference is Claude CLI but the CLI isn\'t currently detected (not installed or OAuth expired), so runtime actually routes to Native. Use the Install button on the Claude CLI card below - or pick Native as your default instead.',
  claudeCli: 'Claude CLI',
  claudeCliTag: 'Anthropic official CLI',
  claudeCliPitch: 'Runs the Agent through Anthropic\'s official CLI. Fully compatible with the Claude Code ecosystem - ~/.claude/settings.json, hooks, and MCP servers all work as-is.',
  codexCli: 'Codex CLI',
  codexCliTag: 'OpenAI Codex app-server',
  codexCliPitch: 'Routes through the Codex app-server for Codex Account models (gpt-5.5 etc., quota covered by your ChatGPT plan), and also serves configured Natives providers via the provider proxy.',
  nativeRuntime: 'Native Engine',
  nativeTag: 'Natives built-in',
  nativePitch: 'Natives calls provider APIs directly. Built for multi-provider, observable, recoverable runs - context and permissions stay inside Natives, no external CLI required.',
  installed: 'Installed',
  installedV: 'Installed v',
  notInstalled: 'Not installed - selecting it falls back to Native',
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
  codexInspectionUnavailable: 'Codex account and model inspection are not wired to a real app-server API yet. This version only shows CLI availability to avoid untrusted account data.',
  loadRuntimeFailed: 'Failed to load execution engine settings',
  saveRuntimeFailed: 'Failed to save execution engine settings',
  detectRuntimeFailed: 'CLI detection failed',
  tasksLoadFailed: 'Failed to load scheduled tasks',
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
  taskName: 'Task name',
  taskPrompt: 'Prompt',
  taskSchedule: 'Schedule',
  taskEnabled: 'Enabled',
  taskNextRun: 'Next run',
  detect: 'Redetect environment',
  detectingBtn: 'Detecting…',
  cliConfig: 'Claude CLI config boundary',
  cliConfigDesc: 'AiNative detects whether Claude CLI is available and invokes it through a controlled child process at runtime. This version does not edit ~/.claude/settings.json or modify your shell, terminal, or global Claude configuration.',
  cliConfigUserOwned: 'User-managed',
  cliConfigUserOwnedDesc: 'Permissions, hooks, MCP servers, and OAuth login remain owned by Claude CLI. Change them through the official Claude CLI when needed.',
  cliConfigSandbox: 'AiNative sandbox',
  cliConfigSandboxDesc: 'AiNative only manages its own Runtime, Provider, Key, and terminal sandbox settings. Keys stay encrypted in the backend and are not written into Claude global config.',
};

function tt(locale: Locale, key: string): string {
  return locale.startsWith('zh') ? (ZH as Record<string, string>)[key] ?? key : EN[key] ?? key;
}

// ═══════════════════════════════════════════════════════════════
// RuntimeStatusPill — 5 态状态胶囊
// ═══════════════════════════════════════════════════════════════

function RuntimeStatusPill({ state, locale }: { state: RuntimeState; locale: Locale }) {
  const tone: Record<RuntimeState, string> = {
    selected: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20',
    available: 'bg-zinc-500/10 text-[var(--text-secondary)] border border-zinc-500/15',
    degraded: 'bg-amber-500/15 text-amber-600 dark:text-amber-400 border border-amber-500/20',
    blocked: 'bg-red-500/15 text-red-600 dark:text-red-400 border border-red-500/20',
    disabled: 'bg-zinc-500/10 text-[var(--text-secondary)] border border-zinc-500/15',
  };
  const dot: Record<RuntimeState, string> = {
    selected: 'bg-emerald-500', available: 'bg-zinc-400', degraded: 'bg-amber-500',
    blocked: 'bg-red-500', disabled: 'bg-zinc-400',
  };
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-medium ${tone[state]}`}>
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
    <div className="rounded-lg bg-[var(--surface-subtle)] px-4 py-1 divide-y divide-[var(--border)] border border-[var(--border)]/60">
      {rows.map((r) => (
        <div key={r.label} className="py-2.5 flex items-start justify-between gap-4">
          <span className="text-[11px] font-medium text-[var(--text-secondary)] shrink-0 mt-0.5">{r.label}</span>
          <span className="text-xs text-[var(--text)] text-right leading-relaxed">{r.value}</span>
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
    <div className="rounded-xl bg-[var(--surface)] border border-[var(--border)] p-5 shadow-xs flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2 flex-wrap pb-1 border-b border-[var(--border)]/50">
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
  selected, onSelect, title, tagline, pitch, statusKind, statusText, locale, icon, disabled = false,
}: {
  selected: boolean; onSelect: () => void; title: string; tagline: string; pitch: string;
  statusKind: 'ok' | 'warning' | 'blocked'; statusText: string; locale: Locale; icon: React.ReactNode;
  disabled?: boolean;
}) {
  const isZh = locale.startsWith('zh');
  const handleClick = () => {
    if (!disabled) {
      onSelect();
    }
  };

  return (
    <div
      role="button"
      tabIndex={disabled ? -1 : 0}
      onClick={handleClick}
      onKeyDown={(e) => {
        if (!disabled && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault();
          onSelect();
        }
      }}
      aria-pressed={selected}
      aria-disabled={disabled}
      className={`relative w-full text-left rounded-xl border p-5 flex flex-col gap-3 transition-all ${
        disabled
          ? 'border-[var(--border)] bg-[var(--surface-subtle)] opacity-75 cursor-not-allowed'
          : selected
            ? 'border-[var(--primary)] bg-[var(--primary-soft)] ring-2 ring-[var(--primary)]/30 shadow-sm cursor-pointer'
            : 'border-[var(--border)] bg-[var(--surface)] hover:border-[var(--border-hover)] hover:shadow-sm cursor-pointer'
      }`}
    >
      {/* 顶部右侧：选择标记 (Radio / Checkmark badge) */}
      <div className="absolute top-4 right-4 flex items-center gap-1.5 pointer-events-none">
        {selected ? (
          <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-[var(--primary)] bg-[var(--primary-soft)] border border-[var(--primary)]/30 px-2 py-0.5 rounded-full">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="20 6 9 17 4 12" />
            </svg>
            {isZh ? '当前默认' : 'Default'}
          </span>
        ) : disabled ? (
          <span className="text-[10px] font-medium text-zinc-400 dark:text-zinc-500 bg-zinc-100 dark:bg-zinc-800 px-2 py-0.5 rounded-full">
            {isZh ? '暂未开放' : 'Locked'}
          </span>
        ) : (
          <span className="size-4 rounded-full border-2 border-zinc-300 dark:border-zinc-600 transition-colors" />
        )}
      </div>

      <div className="pr-20 flex items-start gap-3">
        <span className="shrink-0 mt-0.5 p-1.5 rounded-lg bg-[var(--surface)] border border-[var(--border)]">{icon}</span>
        <div className="min-w-0">
          <h4 className={`text-sm font-semibold tracking-tight ${selected ? 'text-[var(--primary)]' : 'text-[var(--text)]'}`}>{title}</h4>
          <p className="text-xs text-[var(--text-secondary)] mt-0.5 font-normal leading-snug">{tagline}</p>
        </div>
      </div>

      <p className="text-xs text-[var(--text-secondary)] leading-relaxed line-clamp-2">{pitch}</p>

      {/* 底部状态：指示圆点（非打钩图标，消除与选择打钩的混淆） */}
      <div className="flex items-center justify-between gap-2 mt-auto pt-2 border-t border-[var(--border)]/50">
        <div className="flex items-center gap-2 text-[11px]">
          <span
            className={`size-2 rounded-full shrink-0 ${
              statusKind === 'ok'
                ? 'bg-emerald-500 shadow-[0_0_6px_rgba(16,185,129,0.4)]'
                : statusKind === 'warning'
                  ? 'bg-amber-500 shadow-[0_0_6px_rgba(245,158,11,0.4)]'
                  : 'bg-zinc-400 dark:bg-zinc-600'
            }`}
          />
          <span
            className={`truncate font-medium ${
              statusKind === 'ok'
                ? 'text-emerald-600 dark:text-emerald-400'
                : statusKind === 'warning'
                  ? 'text-amber-600 dark:text-amber-400'
                  : 'text-[var(--text-secondary)]'
            }`}
          >
            {statusText}
          </span>
        </div>
      </div>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// InfoRow — 键值对行
// ═══════════════════════════════════════════════════════════════

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="py-2.5 flex items-center justify-between gap-3">
      <span className="text-[11px] text-[var(--text-secondary)] shrink-0 font-medium">{label}</span>
      <div className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">{children}</div>
    </div>
  );
}

function InfoBlock({ children }: { children: React.ReactNode }) {
  return <div className="rounded-lg bg-[var(--surface-subtle)] px-4 divide-y divide-[var(--border)] border border-[var(--border)]/60">{children}</div>;
}

function InlineNotice({ tone = 'info', children }: { tone?: 'info' | 'warning' | 'error'; children: React.ReactNode }) {
  const toneClass = tone === 'error'
    ? 'border-red-500/20 bg-red-500/5 text-red-600 dark:text-red-400'
    : tone === 'warning'
      ? 'border-amber-500/20 bg-amber-500/5 text-amber-600 dark:text-amber-400'
      : 'border-blue-500/20 bg-blue-500/5 text-blue-600 dark:text-blue-400';

  return (
    <div className={`rounded-lg border px-3.5 py-2.5 text-xs leading-relaxed ${toneClass}`}>
      {children}
    </div>
  );
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

function CapBadge({ val, isZh }: { val: string; isZh: boolean }) {
  if (val === '✅') {
    return (
      <span className="inline-flex items-center gap-1 text-[11px] font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3"><polyline points="20 6 9 17 4 12"/></svg>
        {isZh ? '支持' : 'Supported'}
      </span>
    );
  }
  if (val === '⚠️') {
    return (
      <span className="inline-flex items-center gap-1 text-[11px] font-medium text-amber-600 dark:text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded-full">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        {isZh ? '部分' : 'Partial'}
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1 text-[11px] font-medium text-zinc-400 dark:text-zinc-500 bg-zinc-500/10 px-2 py-0.5 rounded-full">
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      {isZh ? '暂无' : 'N/A'}
    </span>
  );
}

function CapabilityMatrix({ locale }: { locale: Locale }) {
  const isZh = locale.startsWith('zh');
  const runtimes: { id: RuntimeId; label: string }[] = [
    { id: 'claude_cli', label: tt(locale, 'claudeCli') },
    { id: 'codex_cli', label: tt(locale, 'codexCli') },
    { id: 'native', label: tt(locale, 'nativeRuntime') },
  ];
  return (
    <div className="overflow-x-auto rounded-lg border border-[var(--border)]/60">
      <table className="w-full text-xs border-collapse">
        <thead>
          <tr className="border-b border-[var(--border)] bg-[var(--surface-subtle)]">
            <th className="py-2.5 px-4 text-left text-[var(--text-secondary)] font-semibold">{tt(locale, 'capability')}</th>
            {runtimes.map((r) => <th key={r.id} className="py-2.5 px-4 text-center text-[var(--text-secondary)] font-semibold">{r.label}</th>)}
          </tr>
        </thead>
        <tbody className="divide-y divide-[var(--border)]/40 bg-[var(--surface)]">
          {CAP_ROWS.map((row) => (
            <tr key={row.key} className="hover:bg-[var(--surface-subtle)]/50 transition-colors">
              <td className="py-2.5 px-4 text-[var(--text)] font-medium">{isZh ? row.zh : row.en}</td>
              {runtimes.map((r) => (
                <td key={r.id} className="py-2.5 px-4 text-center">
                  <CapBadge val={CAP_MAP[r.id][row.key]} isZh={isZh} />
                </td>
              ))}
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
    <div className="flex items-center justify-between py-2.5 px-3.5 rounded-lg bg-[var(--surface)] border border-[var(--border)] shadow-xs">
      <div className="flex items-center gap-2">
        <span className="text-xs font-medium text-[var(--text)]">{label}</span>
        {sideEffect && <span className="text-[10px] px-1.5 py-0.5 rounded-md font-medium bg-amber-500/15 text-amber-600 dark:text-amber-400 border border-amber-500/20">{tt(locale, 'sideEffect')}</span>}
      </div>
      <button
        onClick={onToggle}
        role="switch"
        aria-checked={enabled}
        className={`relative w-9 h-5 rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--primary)] ${
          enabled ? 'bg-[var(--primary)]' : 'bg-zinc-300 dark:bg-zinc-600'
        }`}
      >
        <span className={`absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-all ${enabled ? 'left-[18px]' : 'left-0.5'}`} />
      </button>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// Scheduled Tasks Panel
// ═══════════════════════════════════════════════════════════════

function ScheduledTasksPanel({ locale }: { locale: Locale }) {
  const { toast } = useToast();
  const [tasks, setTasks] = useState<ScheduledTask[]>([]);
  const [loading, setLoading] = useState(true);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await window.nativesAPI?.scheduler?.listTasks?.() as ScheduledTask[] | undefined;
        if (cancelled) return;
        setTasks(r ?? []);
        setErrorMessage(null);
      } catch (error) {
        if (cancelled) return;
        const classified = classifyError(error);
        const message = `${tt(locale, 'tasksLoadFailed')}: ${classified.userMessage}`;
        setErrorMessage(`${message}. ${classified.actionHint}`);
        toast(message, 'error');
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [locale, toast]);

  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'tasksDesc')}</p>
      {loading ? <div className="text-xs text-[var(--text-disabled)] py-4 text-center">…</div>
        : errorMessage ? <InlineNotice tone="error">{errorMessage}</InlineNotice>
        : tasks.length === 0 ? <div className="text-xs text-[var(--text-disabled)] py-4 text-center border border-dashed border-[var(--border)] rounded-lg bg-[var(--surface-subtle)]">{tt(locale, 'noTasks')}</div>
        : <div className="flex flex-col gap-2">{tasks.map((t) => (
          <div key={t.id} className="flex items-center justify-between py-2.5 px-3.5 rounded-lg bg-[var(--surface)] border border-[var(--border)]">
            <div className="flex flex-col gap-0.5 min-w-0">
              <span className="text-xs font-semibold text-[var(--text)] truncate">{t.name}</span>
              <span className="text-[11px] text-[var(--text-secondary)] truncate">{t.prompt}</span>
            </div>
            <div className="flex items-center gap-3 shrink-0">
              <span className={`text-[10px] px-2 py-0.5 rounded-full font-medium ${t.enabled ? 'bg-emerald-500/15 text-emerald-600 border border-emerald-500/20' : 'bg-zinc-500/10 text-[var(--text-secondary)]'}`}>{t.enabled ? tt(locale, 'taskEnabled') : 'Off'}</span>
              <span className="text-[11px] text-[var(--text-secondary)]">{t.nextRun}</span>
            </div>
          </div>
        ))}</div>
      }
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// SVG 图标组件
// ═══════════════════════════════════════════════════════════════

const Icons = {
  Check: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" className="text-emerald-500"><polyline points="20 6 9 17 4 12" /></svg>,
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
  const { toast } = useToast();

  // ── Runtime state ──
  const [selectedRuntime, setSelectedRuntime] = useState<RuntimeId>(() => {
    const pref = loadPreferredRuntimeId();
    if (pref === 'claude_cli' || pref === 'codex_cli' || pref === 'native') return pref;
    return 'native';
  });

  const [detecting, setDetecting] = useState(false);
  const [claudeAvailable, setClaudeAvailable] = useState(false);
  const [codexAvailable, setCodexAvailable] = useState(false);
  const [claudeVersion, setClaudeVersion] = useState<string | null>(null);
  const [codexVersion, setCodexVersion] = useState<string | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);

  const selectRuntime = (id: RuntimeId) => {
    // Codex stays unselectable until app-server is real (REQ-T02).
    if (id === 'codex_cli') {
      toast(
        isZh
          ? 'Codex 尚未 available，保持不可选（app-server 未实现）'
          : 'Codex stays unselectable until app-server is available',
        'warning',
      );
      return;
    }
    if (id === 'claude_cli' && !claudeAvailable) {
      toast(
        isZh
          ? '未检测到 Claude CLI，无法设为默认引擎'
          : 'Claude CLI not detected — cannot set as default',
        'warning',
      );
      return;
    }
    setSelectedRuntime(id);
    savePreferredRuntimeId(id);
  };

  // ── Tool settings ──
  const [enabledTools, setEnabledTools] = useState<Record<string, boolean>>(DEFAULT_ENABLED_TOOLS);
  const [maxSelfHeal, setMaxSelfHeal] = useState(3);
  const [maxSteps, setMaxSteps] = useState(50);
  const [executorLoaded, setExecutorLoaded] = useState(false);

  // ── Load data ──
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [rtResult, execResult] = await Promise.all([
          window.nativesAPI?.runtime?.listAvailable?.() as RuntimeMetadata[] | undefined,
          window.nativesAPI?.executorSettings?.get?.() as { enabledTools: Record<string, boolean>; maxSelfHeal: number; maxSteps?: number } | undefined,
        ]);
        if (rtResult) {
          if (cancelled) return;
          setClaudeAvailable(rtResult.some(r => r.id === 'claude_cli' && r.available));
          setCodexAvailable(rtResult.some(r => r.id === 'codex_cli' && r.available));
        }
        if (execResult) {
          if (cancelled) return;
          setEnabledTools(execResult.enabledTools ?? DEFAULT_ENABLED_TOOLS);
          setMaxSelfHeal(execResult.maxSelfHeal ?? 3);
          setMaxSteps(execResult.maxSteps ?? 50);
          setExecutorLoaded(true);
        }
        if (!cancelled) setRuntimeError(null);
      } catch (error) {
        if (cancelled) return;
        const classified = classifyError(error);
        const message = `${tt(locale, 'loadRuntimeFailed')}: ${classified.userMessage}`;
        setRuntimeError(`${message}. ${classified.actionHint}`);
        toast(message, 'error');
      }
    })();
    return () => { cancelled = true; };
  }, [locale, toast]);

  // ── Save executor ──
  const saveExecutor = useCallback(async (next: Record<string, boolean>, selfHeal: number, steps: number) => {
    if (!executorLoaded) return;
    try {
      await window.nativesAPI?.executorSettings?.save?.({ enabledTools: next, maxSelfHeal: selfHeal, maxSteps: steps });
      setRuntimeError(null);
    } catch (error) {
      const classified = classifyError(error);
      const message = `${tt(locale, 'saveRuntimeFailed')}: ${classified.userMessage}`;
      setRuntimeError(`${message}. ${classified.actionHint}`);
      toast(message, 'error');
    }
  }, [executorLoaded, locale, toast]);

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
      setRuntimeError(null);
    } catch (error) {
      const classified = classifyError(error);
      const message = `${tt(locale, 'detectRuntimeFailed')}: ${classified.userMessage}`;
      setRuntimeError(`${message}. ${classified.actionHint}`);
      toast(message, 'error');
    }
    finally { setDetecting(false); }
  }, [locale, toast]);

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
      // Product red line (REQ-T02): app-server not implemented — always blocked.
      return {
        state: 'blocked',
        reason: isZh
          ? 'Codex app-server 尚未实现（检测到二进制也不开放）'
          : 'Codex app-server is not implemented (binary alone does not enable it)',
        impact: isZh ? '无法将 Codex 设为默认引擎；run.start(runtime_id=codex_cli) 会被拒绝' : 'Cannot set Codex as default; run.start(runtime_id=codex_cli) is rejected',
        recovery: isZh ? '使用 Native 或 Claude CLI；待 app-server JSON-RPC 落地后再开放' : 'Use Native or Claude CLI until app-server JSON-RPC ships',
      };
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

  const sectionCard = 'rounded-xl bg-[var(--surface)] border border-[var(--border)] p-5 shadow-xs flex flex-col gap-4';
  const sectionTitle = 'text-xs font-semibold text-[var(--text-secondary)] uppercase tracking-wider';

  return (
    <div className="max-w-4xl mx-auto space-y-8 pb-10">
      {/* ── 1. 页面标题 ── */}
      <div>
        <h2 className="text-xl font-semibold tracking-tight text-[var(--text)]">{tt(locale, 'pageTitle')}</h2>
        <p className="text-xs text-[var(--text-secondary)] mt-1.5 leading-relaxed">{tt(locale, 'pageDesc')}</p>
      </div>

      {runtimeError && <InlineNotice tone="error">{runtimeError}</InlineNotice>}

      {/* ── 2. 默认引擎选择器 ── */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-sm font-semibold text-[var(--text)]">{tt(locale, 'defaultEngine')}</h3>
            <p className="text-xs text-[var(--text-secondary)] mt-0.5">{tt(locale, 'defaultEngineDesc')}</p>
          </div>
          <button
            onClick={handleDetect}
            disabled={detecting}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg text-[var(--primary)] bg-[var(--primary-soft)] hover:bg-[var(--primary)]/15 border border-[var(--primary)]/20 transition-colors disabled:opacity-50 shrink-0"
          >
            <Icons.Refresh />
            {detecting ? tt(locale, 'detectingBtn') : tt(locale, 'detect')}
          </button>
        </div>

        {driftWarning && (
          <div className="rounded-lg border border-amber-500/20 bg-amber-500/5 px-3.5 py-2.5 text-xs text-amber-600 dark:text-amber-400 flex items-start gap-2">
            <span className="shrink-0 mt-0.5"><Icons.Warning /></span>
            <span className="leading-relaxed">{tt(locale, 'driftWarningCliMissing')}</span>
          </div>
        )}

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 pt-1">
          <EnginePickerCard
            selected={selectedRuntime === 'claude_cli'}
            onSelect={() => selectRuntime('claude_cli')}
            title={tt(locale, 'claudeCli')}
            icon={<Icons.Anthropic />}
            tagline={tt(locale, 'claudeCliTag')}
            pitch={tt(locale, 'claudeCliPitch')}
            statusKind={claudeAvailable ? 'ok' : 'warning'}
            statusText={claudeAvailable ? `${tt(locale, 'installedV')}${claudeVersion ?? ''}` : tt(locale, 'notInstalled')}
            locale={locale}
          />
          <EnginePickerCard
            selected={selectedRuntime === 'codex_cli'}
            onSelect={() => selectRuntime('codex_cli')}
            title={tt(locale, 'codexCli')}
            icon={<Icons.OpenAI />}
            tagline={tt(locale, 'codexCliTag')}
            pitch={tt(locale, 'codexCliPitch')}
            statusKind="blocked"
            statusText={isZh ? 'Codex app-server 未接入' : 'App-server locked'}
            locale={locale}
            disabled={true}
          />
          <EnginePickerCard
            selected={selectedRuntime === 'native'}
            onSelect={() => selectRuntime('native')}
            title={tt(locale, 'nativeRuntime')}
            icon={<Icons.Native />}
            tagline={tt(locale, 'nativeTag')}
            pitch={tt(locale, 'nativePitch')}
            statusKind="ok"
            statusText={tt(locale, 'alwaysAvailable')}
            locale={locale}
          />
        </div>
      </div>

      {/* ── 3. "新会话会用什么" 只读解释块 ── */}
      <div className={sectionCard}>
        <div className="flex flex-col gap-1">
          <h3 className="text-sm font-semibold leading-tight text-[var(--text)]">{tt(locale, 'whatNewChatUses')}</h3>
          <p className="text-xs text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'whatNewChatDesc')}</p>
        </div>
        <InfoBlock>
          <InfoRow label={tt(locale, 'runtimeLabel')}>
            <span className="font-semibold text-[var(--primary)]">
              {selectedRuntime === 'claude_cli' ? tt(locale, 'claudeCli') : selectedRuntime === 'codex_cli' ? tt(locale, 'codexCli') : tt(locale, 'nativeRuntime')}
            </span>
          </InfoRow>
          <InfoRow label={tt(locale, 'defaultProvider')}>{tt(locale, 'notConfigured')}</InfoRow>
          <InfoRow label={tt(locale, 'defaultModel')}>{tt(locale, 'notConfigured')}</InfoRow>
          {driftWarning && (
            <InfoRow label={tt(locale, 'fallbackPath')}>
              <span className="font-medium text-amber-600 dark:text-amber-400">{tt(locale, 'cliUnavailableFallback')}</span>
            </InfoRow>
          )}
        </InfoBlock>
      </div>

      {/* ── 4. Claude CLI 详情卡片 ── */}
      <RuntimeCard name={tt(locale, 'claudeCli')} state={getRuntimeStatus('claude_cli').state} locale={locale}>
        <RuntimeStatusExplanation info={getRuntimeStatus('claude_cli')} locale={locale} />
        <InfoBlock>
          <InfoRow label={tt(locale, 'cliStatus')}>
            {claudeAvailable ? (
              <span className="inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400 font-medium">
                <Icons.Check />
                <span className="font-mono text-xs">v{claudeVersion ?? '?'}</span>
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 text-zinc-400 font-medium">
                <Icons.X />
                <span>{tt(locale, 'notInstalledShort')}</span>
              </span>
            )}
            <button onClick={handleDetect} className="p-1 rounded-md hover:bg-[var(--surface)] text-[var(--text-secondary)] hover:text-[var(--text)] transition-colors">
              <Icons.Refresh />
            </button>
          </InfoRow>
        </InfoBlock>
        <details className="rounded-lg bg-[var(--surface-subtle)] px-4 py-2.5 group border border-[var(--border)]/60">
          <summary className="flex items-center justify-between gap-2 cursor-pointer text-xs font-medium select-none list-none text-[var(--text-secondary)] hover:text-[var(--text)]">
            <span className="flex items-center gap-2"><Icons.Code />{tt(locale, 'cliConfig')}</span>
            <span className="transition-transform group-open:rotate-180"><Icons.Chevron /></span>
          </summary>
          <p className="mt-2 mb-3 text-xs text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'cliConfigDesc')}</p>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3">
              <div className="text-xs font-medium text-[var(--text)]">{tt(locale, 'cliConfigUserOwned')}</div>
              <p className="mt-1 text-[11px] leading-relaxed text-[var(--text-secondary)]">{tt(locale, 'cliConfigUserOwnedDesc')}</p>
            </div>
            <div className="rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3">
              <div className="text-xs font-medium text-[var(--text)]">{tt(locale, 'cliConfigSandbox')}</div>
              <p className="mt-1 text-[11px] leading-relaxed text-[var(--text-secondary)]">{tt(locale, 'cliConfigSandboxDesc')}</p>
            </div>
          </div>
        </details>
      </RuntimeCard>

      {/* ── 5. Codex CLI 详情卡片 ── */}
      <RuntimeCard name={tt(locale, 'codexCli')} state={getRuntimeStatus('codex_cli').state} locale={locale}>
        <RuntimeStatusExplanation info={getRuntimeStatus('codex_cli')} locale={locale} />
        <InfoBlock>
          <InfoRow label={tt(locale, 'appServer')}>
            {codexAvailable ? (
              <span className="inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400 font-medium">
                <Icons.Check />
                <span className="font-mono text-xs">{codexVersion ?? ''}</span>
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 text-zinc-400 font-medium">
                <Icons.X />
                <span>{tt(locale, 'notInstalledShort')}</span>
              </span>
            )}
            <button onClick={handleDetect} className="p-1 rounded-md hover:bg-[var(--surface)] text-[var(--text-secondary)] hover:text-[var(--text)] transition-colors">
              <Icons.Refresh />
            </button>
          </InfoRow>
        </InfoBlock>
        <InlineNotice tone="warning">{tt(locale, 'codexInspectionUnavailable')}</InlineNotice>
      </RuntimeCard>

      {/* ── 6. Native 详情卡片 ── */}
      <RuntimeCard name={tt(locale, 'nativeRuntime')} state={getRuntimeStatus('native').state} locale={locale}>
        <RuntimeStatusExplanation info={getRuntimeStatus('native')} locale={locale} />
        <InfoBlock>
          <div className="py-2.5 flex items-start justify-between gap-4">
            <div className="flex flex-col gap-0.5 max-w-[65%]">
              <span className="text-xs font-semibold text-[var(--text)]">{tt(locale, 'capabilities')}</span>
              <span className="text-[11px] text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'capabilitiesDesc')}</span>
            </div>
            <span className="text-[10px] text-[var(--text-disabled)] font-medium bg-[var(--surface)] px-2 py-0.5 rounded-full border border-[var(--border)] shrink-0">{tt(locale, 'shipsWithApp')}</span>
          </div>
          <div className="py-2.5 flex items-start justify-between gap-4">
            <div className="flex flex-col gap-0.5 max-w-[65%]">
              <span className="text-xs font-semibold text-[var(--text)]">{tt(locale, 'permissions')}</span>
              <span className="text-[11px] text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'permissionsDesc')}</span>
            </div>
            <span className="text-[10px] text-[var(--text-disabled)] font-medium bg-[var(--surface)] px-2 py-0.5 rounded-full border border-[var(--border)] shrink-0">{tt(locale, 'perSession')}</span>
          </div>
          <div className="py-2.5 flex items-start justify-between gap-4">
            <div className="flex flex-col gap-0.5 max-w-[65%]">
              <span className="text-xs font-semibold text-[var(--text)]">{tt(locale, 'context')}</span>
              <span className="text-[11px] text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'contextDesc')}</span>
            </div>
            <span className="text-[10px] text-[var(--text-disabled)] font-medium bg-[var(--surface)] px-2 py-0.5 rounded-full border border-[var(--border)] shrink-0">{tt(locale, 'local')}</span>
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
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
          {tools.map(tool => (
            <ToolToggle
              key={tool.key}
              label={tool.label}
              sideEffect={tool.sideEffect}
              enabled={enabledTools[tool.key] ?? false}
              onToggle={() => toggleTool(tool.key)}
              locale={locale}
            />
          ))}
        </div>
      </div>

      <div className={sectionCard}>
        <h3 className={sectionTitle}>{tt(locale, 'selfHealTitle')}</h3>
        <div className="flex flex-col gap-3.5">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-[var(--text)]">{tt(locale, 'maxSelfHeal')}</span>
            <input
              type="number"
              min={1}
              max={10}
              value={maxSelfHeal}
              onChange={(e) => updateMaxSelfHeal(Number(e.target.value))}
              className="w-16 text-xs font-semibold text-[var(--primary)] text-center py-1 px-2 rounded-lg bg-[var(--surface-subtle)] border border-[var(--border)] outline-none focus:ring-2 focus:ring-[var(--primary)]/30"
            />
          </div>
          <p className="text-xs text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'selfHealDesc')}</p>

          <div className="h-px bg-[var(--border)]/60 my-0.5" />

          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-[var(--text)]">{tt(locale, 'maxSteps')}</span>
            <input
              type="number"
              min={10}
              max={200}
              value={maxSteps}
              onChange={(e) => updateMaxSteps(Number(e.target.value))}
              className="w-16 text-xs font-semibold text-[var(--primary)] text-center py-1 px-2 rounded-lg bg-[var(--surface-subtle)] border border-[var(--border)] outline-none focus:ring-2 focus:ring-[var(--primary)]/30"
            />
          </div>

          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-[var(--text)]">{tt(locale, 'circuitBreaker')}</span>
            <span className="text-xs text-emerald-600 dark:text-emerald-400 font-medium px-2 py-0.5 rounded-full bg-emerald-500/10 border border-emerald-500/20">{tt(locale, 'circuitBreakerEnabled')}</span>
          </div>

          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-[var(--text)]">{tt(locale, 'doomLoop')}</span>
            <span className="text-xs text-[var(--text-secondary)] font-medium">3 {isZh ? '次' : 'times'}</span>
          </div>
          <p className="text-xs text-[var(--text-secondary)] leading-relaxed">{tt(locale, 'doomLoopDesc')}</p>
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
