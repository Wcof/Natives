// T19 · Preview composition（唯一组装者）
//
// 本文件是 builtin registry / default PreviewContext 的唯一组装点。
// Leaf（T12–T17, T20）只导出 provider/renderer；T19 把它们注册进统一 registry，
// 并把业务 prepare 逻辑留在 provider，不在此重写。
// HTML lane（T20 → Host prepare → /fs/ 授权路由）已合入：html provider 与
// markdown 同级注册，无 H0=BLOCKED 占位。

import type { PreviewContext, PreviewProvider, VirtualFileViewHandle } from './contracts';
import { PreviewRegistry } from './registry';
import { createPreviewContext } from './context';
import { markdownProvider } from './providers/markdown';
import { htmlProvider } from './providers/html';
import { jsonProvider } from './providers/json';
import { codeProvider } from './providers/code';
import { mediaProvider } from './providers/media';
import { pdfProvider } from './providers/pdf';
import { csvProvider } from './providers/csv';
import { archiveProvider } from './providers/archive';

/** 内置 provider 列表（HTML 已注册；无 BLOCKED 分支） */
export const BUILTIN_PROVIDERS: PreviewProvider[] = [
  markdownProvider,
  htmlProvider,
  jsonProvider,
  codeProvider,
  mediaProvider,
  pdfProvider,
  csvProvider,
  archiveProvider,
];

export function createBuiltinRegistry(): PreviewRegistry {
  const registry = new PreviewRegistry();
  for (const provider of BUILTIN_PROVIDERS) {
    registry.register(provider);
  }
  return registry;
}

export function createDefaultContext(): PreviewContext {
  return createPreviewContext();
}

export type { PreviewContext, VirtualFileViewHandle };
