/**
 * menubar-bridge — 菜单栏浮窗 → Host 命令桥（轻量 surface 专用）。
 *
 * 冻结契约命令名（native_menubar_lifecycle 子代理在 src-tauri/src/commands/menubar.rs
 * 注册）：`menubar_hide` / `menubar_open_main` / `menubar_quit`。
 *
 * 本桥优先走 `window.nativesAPI.menubar` facade（若生命周期子代理已暴露），
 * 否则回退到 `window.__nativesCmd`（tauri-adapter 装配时暴露的唯一 invoke helper）。
 * 两条路径都经既有收口，业务组件不直接触碰 @tauri-apps/*（ARCH-004）。
 * 在浏览器开发模式（无 Tauri）下静默 no-op。
 */

type MenubarCommand = 'menubar_hide' | 'menubar_open_main' | 'menubar_quit';

export type MenubarFacade = {
  hide?: () => Promise<unknown> | unknown;
  openMain?: () => Promise<unknown> | unknown;
  quit?: () => Promise<unknown> | unknown;
};

const FACADE_METHOD: Record<MenubarCommand, keyof MenubarFacade> = {
  menubar_hide: 'hide',
  menubar_open_main: 'openMain',
  menubar_quit: 'quit',
};

export async function invokeMenubar(command: MenubarCommand): Promise<void> {
  try {
    const facade = (window.nativesAPI as unknown as { menubar?: MenubarFacade })?.menubar;
    const method = facade?.[FACADE_METHOD[command]];
    if (typeof method === 'function') {
      await method();
      return;
    }
  } catch {
    // Fall through to the raw helper below.
  }
  try {
    await window.__nativesCmd?.(command).catch(() => {});
  } catch {
    // Browser dev mode — no Tauri commands.
  }
}
