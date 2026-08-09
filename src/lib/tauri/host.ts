/**
 * tauri/host — 基座 Shell 域 facade（ARCH-002）
 *
 * 应用级 / 壳层 / 环境 / 通知 / 窗口 / 桥接等基座能力统一入口。
 * 业务组件只允许经本 facade 访问；唯一 raw invoke 在 ./core.ts。
 */

import { cmd, subscribe } from './core';
import type { NativesAPI } from './types';

  // App
export const app: NativesAPI['app'] = {
    version: () => cmd<string>('app_version'),
};

  // DB
export const db: NativesAPI['db'] = {
    get: async (key: string) => {
      const res = await cmd<{ value: unknown } | unknown>('db_get', { key });
      return res && typeof res === 'object' && 'value' in (res as object) ? (res as { value: unknown }).value : res;
    },
    set: (key: string, value: unknown) => cmd('db_set', { key, value }),
    delete: (key: string) => cmd('db_delete', { key }),
    list: (prefix?: string) => cmd('db_list', { prefix }),
};

  // Builtin Tool Registry
export const builtinTool: NativesAPI['builtinTool'] = {
    list: () => cmd<Array<{ id: string; enabled: boolean; driver: string }>>('builtin_tool_list'),
    update: (id: string, enabled: boolean, driver: string) =>
      cmd('builtin_tool_update', { id, enabled, driver }),
    seed: (id: string, driver: string) => cmd('builtin_tool_seed', { id, driver }),
    detect: (driver: string) => cmd<boolean>('builtin_tool_detect', { driver }),
    launch: (driver: string) => cmd('builtin_tool_launch', { driver }),
    ghosttyIsRunning: () => cmd<boolean>('builtin_tool_ghostty_is_running'),
    ghosttyFocus: () => cmd('builtin_tool_ghostty_focus'),
    ghosttyLaunch: (configPath?: string) =>
      cmd('builtin_tool_ghostty_launch', { config_path: configPath }),
    ghosttySyncTheme: () => cmd<string>('builtin_tool_ghostty_sync_theme'),
    ghosttyVtAvailable: () => cmd<boolean>('ghostty_vt_available'),
};

  // Environment
export const env: NativesAPI['env'] = {
    getVariables: (profileId: string) => cmd('env_get_variables', { profileId }),
    getDefaultProfile: () => cmd('env_get_default_profile'),
    listProfiles: () => cmd('env_list_profiles'),
    createProfile: (name: string) => cmd('env_create_profile', { name }),
    deleteProfile: (name: string) => cmd('env_delete_profile', { name }),
    setDefaultProfile: (name: string) => cmd('env_set_default_profile', { name }),
    setVariable: (profileId: string, key: string, value: string) =>
      cmd('env_set_variable', { profileId, key, value }),
    deleteVariable: (profileId: string, key: string) =>
      cmd('env_delete_variable', { profileId, key }),
    encrypt: (text: string) => cmd('env_encrypt', { text }),
};

  // Shell
export const shell: NativesAPI['shell'] = {
    showItemInFolder: (filePath: string) => cmd('show_item_in_folder', { path: filePath }),
    openPath: (filePath: string) => cmd('open_path', { path: filePath }),
};

  // Notifications
export const notification: NativesAPI['notification'] = {
    send: (title: string, body: string, level?: string) =>
      cmd('notification_send', { title, body, level: level || 'info' }),
    list: (unreadOnly?: boolean) => cmd('notification_list', { unreadOnly }),
    markRead: (id: number) => cmd('notification_mark_read', { id }),
    markAllAsRead: () => cmd('notification_mark_all_read'),
};

  // State Persistence
export const state: NativesAPI['state'] = {
    save: (moduleId: string, state: string) => cmd('state_save', { moduleId, state }),
    load: (moduleId: string) => cmd('state_load', { moduleId }),
    clear: (moduleId: string) => cmd('state_clear', { moduleId }),
};

  // Screenshot
export const screenshot: NativesAPI['screenshot'] = {
    watch: (callback) => {
      const stop = subscribe<string>('screenshot:detected', (payload) => callback(payload));
      // Start watching
      cmd('screenshot_start_watching').catch(() => {});
      return () => {
        cmd('screenshot_stop_watching').catch(() => {});
        stop();
      };
    },
    saveAnnotated: (dataUrl: string, targetPath?: string) =>
      cmd('screenshot_save_annotated', { dataUrl, targetPath }),
};

  // Release
export const release: NativesAPI['release'] = {
    inspect: (projectPath: string) => cmd('release_inspect', { projectPath }),
    prepare: (projectPath: string, version: string) =>
      cmd('release_prepare', { projectPath, version }),
    getSequence: (projectPath: string, version: string) =>
      cmd('release_get_sequence', { projectPath, version }),
    execute: (projectPath: string, command: string) =>
      cmd('release_execute', { projectPath, command }),
};

  // Update
export const update: NativesAPI['update'] = {
    check: () => cmd('update_check'),
    mute: (version: string) => cmd('update_mute', { version }),
    dismiss: (version: string) => cmd('update_dismiss', { version }),
    getMuted: () => cmd('update_get_muted'),
    getDismissed: () => cmd('update_get_dismissed'),
};

  // Clipboard
export const clipboard: NativesAPI['clipboard'] = {
    write: (text: string) => cmd('clipboard_write', { text }),
    read: () => cmd('clipboard_read'),
};

  // CodeGraph
export const codegraph: NativesAPI['codegraph'] = {
    read: () => cmd('read_codegraph'),
    rtkGain: () => cmd('rtk_gain'),
};

  // Dialog （文件/目录选择，经 Tauri dialog plugin）
export const dialog: NativesAPI['dialog'] = {
    pickDirectory: async () => {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ directory: true, multiple: false });
        return selected as string | null;
      } catch {
        return null;
      }
    },
    pickFiles: async () => {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ directory: false, multiple: true });
        if (!selected) return [];
        return Array.isArray(selected) ? selected : [selected];
      } catch {
        return [];
      }
    },
    saveFile: async () => {
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const selected = await save();
        return selected as string | null;
      } catch {
        return null;
      }
    },
};

  // Window Controls
export const windowControls: NativesAPI['windowControls'] = {
    minimize: () => cmd('window_minimize'),
    maximize: () => cmd('window_maximize'),
    toggleFullscreen: () => cmd('window_toggle_fullscreen'),
    close: () => cmd('window_close'),
    isMaximized: () => cmd('window_is_maximized'),
    isFullscreen: () => cmd('window_is_fullscreen'),
    tileWindow: (action: string) => cmd('window_tile', { action }),
};

  // Bridge / Security
export const bridge: NativesAPI['bridge'] = {
    getHttpPort: () => cmd<number>('get_http_port'),
    generateToken: (moduleId: string) => cmd<string>('generate_token', { moduleId }),
    validateToken: (token: string, moduleId: string) => cmd<boolean>('validate_token', { token, moduleId }),
};

  // FsWatch — file system change notifications
export const fsWatch: NativesAPI['fsWatch'] = {
    start: (path: string) => cmd<void>('fs_watch_start', { path }),
    stop: (path: string) => cmd<void>('fs_watch_stop', { path }),
    stopAll: () => cmd<void>('fs_watch_stop_all'),
    list: () => cmd<string[]>('fs_watch_list'),
    onChange: (callback: (event: { path: string; kind: string }) => void) =>
      subscribe<{ path: string; kind: string }>('fs-watch-change', (payload) => callback(payload)),
};

  // HtmlPreview — sandboxed HTML preview with local resource rewriting
export const htmlPreview: NativesAPI['htmlPreview'] = {
    prepare: (htmlPath: string) => cmd<{ content: string; fsBase: string; serverPort: number }>('html_preview_prepare', { htmlPath }),
};

  // LidGuard — prevent macOS sleep while terminals are active
export const lidGuard: NativesAPI['lidGuard'] = {
    set: (on: boolean) => cmd<void>('lid_guard_set', { on }),
    status: () => cmd<{ sleepDisabled: boolean; terminalCount: number }>('lid_guard_status'),
};

  // WeChat ClawBot
export const wechat: NativesAPI['wechat'] = {
    env: () => cmd<{ target: string; cwd: string; persona: string; state: string; connected: boolean }>('wechat_env'),
    login: () => cmd<{ qrcode: string; qrcode_img_content: string; state: string }>('wechat_login'),
    pollLogin: (qrcode: string, verifyCode?: string) => cmd<{ state: string; error?: string }>('wechat_poll_login', { qrcode, verifyCode }),
    disconnect: () => cmd<{ ok: boolean }>('wechat_disconnect'),
    check: () => cmd<{ ok: boolean; state: string }>('wechat_check'),
    send: (text: string) => cmd<{ ok: boolean; cid: string }>('wechat_send', { text }),
    setTarget: (target: string) => cmd('wechat_set_target', { target }),
    setCwd: (dir: string) => cmd('wechat_set_cwd', { dir }),
    setPersona: (persona: string) => cmd('wechat_set_persona', { persona }),
    detectAgents: () => cmd<{ claude: boolean; codex: boolean }>('wechat_detect_agents'),
    status: () => cmd<{ state: string; connected: boolean; target: string; cwd: string }>('wechat_status'),
};

  // Plugins
export const plugins: NativesAPI['plugins'] = {
    detect: (name: string) => cmd<string | null>('plugin_detect', { name }),
    install: (name: string) => cmd<void>('plugin_install', { name }),
    uninstall: (name: string) => cmd<void>('plugin_uninstall', { name }),
};

  // Sidecar supervisor (production UDS health; no silent embedded fallback)
export const daemonSupervisor: NativesAPI['daemonSupervisor'] = {
    status: () => cmd('daemon_supervisor_status'),
    ensure: () => cmd('daemon_supervisor_ensure'),
    poll: () => cmd('daemon_supervisor_poll'),
    shutdown: () => cmd('daemon_supervisor_shutdown'),
};

  // Runtime abstraction（Slice B）
export const runtime: NativesAPI['runtime'] = {
    listAvailable: () => cmd('runtime_list_available'),
    detectCli: () => cmd('runtime_detect_cli'),
};

  // ── Capability secrets (ADR-0016 决策 7) ──
export const capabilitySecret: NativesAPI['capabilitySecret'] = {
    set: (data: { kind: 'mcp_env' | 'mcp_bearer' | 'mcp_oauth_refresh'; ownerRef: string; keyName?: string; plaintext: string }) =>
      cmd<{ id: string }>('capability_secret_set', { input: data }),
    delete: (id: string) => cmd<void>('capability_secret_delete', { id }),
    list: (ownerRef: string) =>
      cmd<Array<{ id: string; kind: string; keyName: string | null; createdAt: string }>>('capability_secret_list', { ownerRef }),
};

  // ── MCP OAuth 浏览器流 (ADR-0016 决策 7) ──
export const mcpOauth: NativesAPI['mcpOauth'] = {
    start: (data: { serverId: string; authorizeUrl: string; tokenUrl: string; clientId: string; scopes?: string[]; redirectPort?: number }) =>
      cmd<{ ok: boolean; hasRefresh: boolean }>('mcp_oauth_start', { input: data }),
};

  // FOUC Guard
export const themeReady: NativesAPI['themeReady'] = () => {
    // Tauri: emit event to signal theme readiness
    cmd('theme_ready_signal').catch(() => {
      // Graceful — window show is controlled by Tauri, not Electron
    });
};

  // Theme
export const getTheme: NativesAPI['getTheme'] = () => cmd('get_theme');

export const setTheme: NativesAPI['setTheme'] = (theme: string) => cmd('set_theme', { theme });

  // Locale
export const getLocale: NativesAPI['getLocale'] = () => cmd('get_locale');

export const setLocale: NativesAPI['setLocale'] = async (locale: string) => {
    window.dispatchEvent(new CustomEvent('locale-changed', { detail: locale }));
    await cmd('set_locale', { locale });
};

  // DB State Changed event
export const onDbStateChanged: NativesAPI['onDbStateChanged'] = (callback) =>
    subscribe<{ channel: string; data: unknown }>('db-state-changed', (payload) =>
      callback(payload, payload.channel, payload.data),
    );

  // Widget window
export const openWidgetWindow: NativesAPI['openWidgetWindow'] = () => {
    cmd('open_widget_window').catch(() => {});
};

// ── macOS menubar popup (frozen contract: commands/menubar.rs) ──
// Window label `menubar`, route `?surface=menubar`. Every command is validated
// against the invoking window label in Rust; the popup has its own minimal
// capability (capabilities/menubar.json) and never inherits main's shell/fs/
// dialog/credential/Workshop permissions.
export const menubar: NativesAPI['menubar'] = {
  toggle: () => cmd('menubar_toggle').catch(() => {}),
  hide: () => cmd('menubar_hide').catch(() => {}),
  openMain: () => cmd('menubar_open_main').catch(() => {}),
  openPersonalOverview: () => cmd('menubar_open_personal_overview').catch(() => {}),
  quit: () => cmd('menubar_quit').catch(() => {}),
};

