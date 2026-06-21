/**
 * Builtin Tool Registry — pure data, zero runtime overhead.
 *
 * Each tool is a declarative record. Sidebar / Settings / ShellLayout
 * all read from this registry + DB state to render UI dynamically.
 * Adding a new tool = adding one entry here + i18n keys + Rust match arm.
 *
 * NOTE: Terminal 只有原生模式（启用/关闭），无驱动选择。
 * Ghostty VT 状态解析层（libghostty-vt）在编译期内置在原生终端中，
 * 不是运行时选项。外部 Ghostty app 作为独立的 Builtin Tool 存在。
 *
 * Unenabled tools cost 0 bytes in initial bundle and 0 memory at runtime.
 */

export interface BuiltinToolDriver {
  id: string;
  label: { zh: string; en: string };
}

export interface BuiltinTool {
  id: string;
  icon: string;             // lucide icon name: 'Terminal' | 'Code2' | 'Globe'
  label: { zh: string; en: string };
  /** 驱动列表（仅编辑器/浏览器等需要选择外部工具的类型使用） */
  drivers?: BuiltinToolDriver[];
  defaultDriver?: string;
  /** Lazy component path for embedded panel — only loaded when activated */
  componentPath?: string;   // e.g. 'shell/Terminal'
  /**
   * Special routing: some tools don't occupy main content area.
   * 'panel-bottom' = bottom panel (like terminal), 'content' = main area
   */
  displayMode: 'panel-bottom' | 'content';
}

export const BUILTIN_TOOLS: readonly BuiltinTool[] = [
  {
    id: 'terminal',
    icon: 'Terminal',
    label: { zh: '终端', en: 'Terminal' },
    // 无 drivers 列表 = 仅原生，无驱动选择，只有启用/关闭
    componentPath: 'shell/Terminal',
    displayMode: 'panel-bottom',
  },
  // ── Ghostty 外部终端 app（独立工具，不是 terminal 的驱动）──
  {
    id: 'ghostty',
    icon: 'Terminal',
    label: { zh: 'Ghostty', en: 'Ghostty' },
    displayMode: 'content',
  },
  // ── Future tools: add one entry each ──
  // {
  //   id: 'editor',
  //   icon: 'Code2',
  //   label: { zh: '编辑器', en: 'Editor' },
  //   drivers: [
  //     { id: 'native', label: { zh: '原生', en: 'Native' } },
  //     { id: 'vscode', label: { zh: 'VS Code', en: 'VS Code' } },
  //   ],
  //   defaultDriver: 'native',
  //   componentPath: 'shell/EditorPanel',
  //   displayMode: 'content',
  // },
  // {
  //   id: 'browser',
  //   icon: 'Globe',
  //   label: { zh: '浏览器', en: 'Browser' },
  //   drivers: [
  //     { id: 'native', label: { zh: '原生', en: 'Native' } },
  //     { id: 'chromium', label: { zh: 'Chromium', en: 'Chromium' } },
  //   ],
  //   defaultDriver: 'native',
  //   componentPath: 'shell/BrowserPanel',
  //   displayMode: 'content',
  // },
] as const;

/** Lookup helper — O(n) but n is tiny (<10) */
export function getBuiltinTool(id: string): BuiltinTool | undefined {
  return BUILTIN_TOOLS.find((t) => t.id === id);
}

/**
 * Seed all builtin tools into the DB (idempotent — INSERT OR IGNORE).
 * Call once on app startup, then from any component that loads tool states.
 * This ensures the DB always has rows for all registered tools.
 */
export async function seedAllBuiltinTools(): Promise<void> {
  const api = (window as any).nativesAPI;
  if (!api?.builtinTool?.seed) return;
  for (const tool of BUILTIN_TOOLS) {
    try {
      await api.builtinTool.seed(tool.id, tool.defaultDriver || 'native');
    } catch { /* idempotent — ignore on conflict */ }
  }
}
