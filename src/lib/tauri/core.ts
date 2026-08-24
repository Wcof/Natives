/**
 * tauri/core — 唯一 raw invoke 核心（ARCH-002）
 *
 * 全仓只有本文件允许直接 import `invoke` / `listen` / `convertFileSrc` 等
 * Tauri 底层 API。所有 domain facade（files / execution-engine / provider /
 * module / terminal / creative / jobs / usage / host）都必须经 `cmd` 发命令。
 * 业务组件禁止直接触碰 @tauri-apps/*（ARCH-004）。
 */

import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** 统一命令执行：把 Tauri invoke 的失败归一为带命令名的 Error。 */
export async function cmd<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    const msg = typeof err === 'string' ? err : String(err);
    throw new Error(`Tauri command failed: ${command} — ${msg}`);
  }
}

/** Tauri 事件订阅（listen 的轻封装，统一清理语义，幂等防御）。 */
export function subscribe<T>(event: string, handler: (payload: T) => void): () => void {
  let unsubscribed = false;
  let unlistenFn: (() => void | Promise<void>) | null = null;

  listen<T>(event, (event) => handler(event.payload))
    .then((fn) => {
      if (unsubscribed) {
        try {
          const res: unknown = fn();
          if (res && typeof (res as Promise<void>).catch === 'function') {
            (res as Promise<void>).catch(() => {});
          }
        } catch {
          // ignore already-unregistered / unlisten errors
        }
      } else {
        unlistenFn = fn;
      }
    })
    .catch(() => {});

  return () => {
    if (unsubscribed) return;
    unsubscribed = true;
    if (unlistenFn) {
      try {
        const res: unknown = unlistenFn();
        if (res && typeof (res as Promise<void>).catch === 'function') {
          (res as Promise<void>).catch(() => {});
        }
      } catch {
        // ignore already-unregistered / unlisten errors
      }
      unlistenFn = null;
    }
  };
}

/** convertFileSrc 转发（fs facade 使用）。 */
export { convertFileSrc };
