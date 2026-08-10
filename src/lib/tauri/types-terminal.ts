/**
 * tauri/types-terminal — Terminal domain 共享类型（ARCH-002 split）
 *
 * 终端 render-state 等 wire payload 声明于此；terminal facade（./terminal.ts）
 * 与业务组件从这里取类型。
 */

export interface RenderStatePayload {
  sessionId: string;
  cursorX: number;
  cursorY: number;
  title: string | null;
  pwd: string | null;
  cols: number;
  rows: number;
}
