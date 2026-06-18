import type { Clipboard } from 'electron';

// 尝试加载 Electron clipboard，测试环境下可能不可用
let clipboard: Clipboard | null = null;
try {
  clipboard = require('electron').clipboard;
} catch {
  // 测试环境下 Electron 不可用
}

/**
 * 验证剪贴板数据是否有效
 */
export function validateClipboardData(data: string | null | undefined): boolean {
  if (!data) return false;
  return data.length > 0;
}

/**
 * 复制文本到剪贴板（通过 Electron clipboard API）
 */
export function copyToClipboard(text: string): boolean {
  if (!text) return false;
  try {
    if (clipboard) {
      clipboard.writeText(text);
      return true;
    }
    return false;
  } catch {
    return false;
  }
}

/**
 * 从剪贴板读取文本
 */
export function readFromClipboard(): string {
  try {
    if (clipboard) {
      return clipboard.readText();
    }
    return '';
  } catch {
    return '';
  }
}
