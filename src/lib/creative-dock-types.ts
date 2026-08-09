/**
 * Creative dock tab type (W4): moved out of `CreativeDock.tsx` so hooks never
 * depend on component internals. The component re-exports it for compatibility.
 */
import type { CreativeAppSummary, CreativeAppWindow } from './tauri-adapter';

export interface CreativeDockTab {
  /** Stable key: `win:{windowId}` when the app has a window, else `app:{appId}`. */
  key: string;
  app: CreativeAppSummary;
  /** The window this tab represents, or null when the app has none yet. */
  window: CreativeAppWindow | null;
  /** Displayed state derived from the real window snapshot. */
  state: 'open' | 'minimized' | 'no-window';
}
