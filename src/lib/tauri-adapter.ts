/**
 * Tauri adapter — barrel（ARCH-002 / ARCH-004 收口）
 *
 * 唯一 raw invoke 在 `./tauri/core`；各 domain facade 位于 `./tauri/<domain>`。
 * 本文件只做装配：重导出全部类型与 facade（保持向后兼容），并组装 `nativesAPI`
 * 默认导出（`window.nativesAPI`）。
 *
 * 业务组件应按域 import `@/lib/tauri/<domain>`（files / terminal / module /
 * provider / execution-engine / jobs / usage / creative / host / assistant），
 * 禁止直接触碰本 barrel 以外的 Tauri 底层 API（ARCH-004）。
 */

// ── 向后兼容：类型与 facade 重导出 ──
export * from './tauri/types';
export { cmd, subscribe, convertFileSrc } from './tauri/core';
export { normalizeProviderTest } from './tauri/provider';
export { terminal } from './tauri/terminal';
export { module } from './tauri/module';
export { creativeApp, creativeDraft } from './tauri/creative';
export { fs, archive, search, git, disk, thumbnail } from './tauri/files';
export { provider, providerRouting } from './tauri/provider';
export { executionEngine } from './tauri/execution-engine';
export { jobs } from './tauri/jobs';
export { usage } from './tauri/usage';
export {
  app,
  db,
  env,
  shell,
  notification,
  state,
  screenshot,
  release,
  update,
  clipboard,
  codegraph,
  dialog,
  windowControls,
  bridge,
  fsWatch,
  htmlPreview,
  lidGuard,
  wechat,
  plugins,
  daemonSupervisor,
  runtime,
  capabilitySecret,
  mcpOauth,
  themeReady,
  getTheme,
  setTheme,
  getLocale,
  setLocale,
  onDbStateChanged,
  openWidgetWindow,
  builtinTool,
  menubar,
} from './tauri/host';
export { agent, skills, library, subagent, assistantV2, project } from './tauri/assistant';

// ── 装配 nativesAPI（唯一实例）──
import { cmd } from './tauri/core';
import type { NativesAPI } from './tauri/types';
import { terminal } from './tauri/terminal';
import { module } from './tauri/module';
import { creativeApp, creativeDraft } from './tauri/creative';
import { fs, archive, search, git, disk, thumbnail } from './tauri/files';
import { provider, providerRouting } from './tauri/provider';
import { executionEngine } from './tauri/execution-engine';
import { jobs } from './tauri/jobs';
import { usage } from './tauri/usage';
import {
  app,
  db,
  env,
  shell,
  notification,
  state,
  screenshot,
  release,
  update,
  clipboard,
  codegraph,
  dialog,
  windowControls,
  bridge,
  fsWatch,
  htmlPreview,
  lidGuard,
  wechat,
  plugins,
  daemonSupervisor,
  runtime,
  capabilitySecret,
  mcpOauth,
  themeReady,
  getTheme,
  setTheme,
  getLocale,
  setLocale,
  onDbStateChanged,
  openWidgetWindow,
  builtinTool,
  menubar,
} from './tauri/host';
import { agent, skills, library, subagent, assistantV2, project } from './tauri/assistant';

export const nativesAPI: NativesAPI = {
  // FOUC Guard
  themeReady,

  // App
  app,

  // DB
  db,

  // Terminal
  terminal,

  // Builtin Tool Registry
  builtinTool,

  // Module
  module,

  // Creative App (multi-source)
  creativeApp,

  // Creative drafts — the creation loop before a module exists
  creativeDraft,

  // Environment
  env,

  // Theme
  getTheme,
  setTheme,

  // Shell
  shell,

  // Locale
  getLocale,
  setLocale,

  // Notifications
  notification,

  // File System
  fs,

  // Archive
  archive,

  // Search
  search,

  // State Persistence
  state,

  // Git
  git,

  // Disk
  disk,

  // Thumbnail
  thumbnail,

  // Agent
  agent,

  // Skills
  skills,

  // DB State Changed event
  onDbStateChanged,

  // Screenshot
  screenshot,

  // Release
  release,

  // Update
  update,

  // Clipboard
  clipboard,

  // Usage
  usage,

  // CodeGraph
  codegraph,

  // Dialog （文件/目录选择，经 Tauri dialog plugin）
  dialog,

  // Provider (unified API — single source of truth)
  provider,

  providerRouting,

  // Assistant — Daemon-canonical (T202/T302): legacy assistant_* CRUD commands
  // retired from registration; conversations/messages live in the Agent Daemon.
  // The frontend talks to the daemon via assistant-gateway/daemon-adapter, not
  // through retired Host commands. No adapter surface is kept for them.

  // Sidecar supervisor (production UDS health; no silent embedded fallback)
  daemonSupervisor,

  // MIG-002: executor_get_settings / executor_save_settings 已物理注销（见 lib.rs），
  // 旧 executorSettings facade 不保留。

  // Execution Engine settings V2（唯一持久化权威 — A6 backend）
  executionEngine,

  // Runtime abstraction（Slice B）
  runtime,

  // Job module（任务）— 契约 v1：入参 JSON snake_case，与后端命令面一致
  jobs,

  // Window Controls
  windowControls,

  // Widget window
  openWidgetWindow,

  // Bridge / Security
  bridge,

  // FsWatch — file system change notifications
  fsWatch,

  // HtmlPreview — sandboxed HTML preview with local resource rewriting
  htmlPreview,

  // LidGuard — prevent macOS sleep while terminals are active
  lidGuard,

  // WeChat ClawBot
  wechat,
  // Plugins
  plugins,

  // ── Library (fanbox clone — G4) ──
  library,

  // ── Subagent (G8) ──
  subagent,

  // ── Capability secrets (ADR-0016 决策 7) ──
  capabilitySecret,

  // ── MCP OAuth 浏览器流 (ADR-0016 决策 7) ──
  mcpOauth,

  // Assistant in-process RPC (no daemon sidecar)
  assistantV2,

  // Project directory management
  project,

  // macOS menubar popup
  menubar,
};

// Expose to window (replaces contextBridge.exposeInMainWorld)
if (typeof window !== 'undefined') {
  window.nativesAPI = nativesAPI;
  // Expose cmd helper so DaemonClient and other modules can invoke
  // Tauri commands without importing @tauri-apps/api/core directly.
  window.__nativesCmd = cmd;
}

export default nativesAPI;
