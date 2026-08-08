// T19 · Preview composition（唯一组装者）
//
// 本文件是 builtin registry / default PreviewContext 的唯一组装点。
// Leaf（T12–T17）只导出 provider/renderer；T19 把它们注册进统一 registry，
// 并把业务 prepare 逻辑留在 provider，不在此重写。
// H0=BLOCKED 时 HTML provider 缺席，由 PreviewRenderer 以 unsupported 呈现；
// HTML lane（T20→T21→T15）完成后再由 T19 一次性追加，不提前占位。

import type { PreviewContext, PreviewProvider, VirtualFileViewHandle } from './contracts';
import { PreviewRegistry } from './registry';
import { createPreviewContext } from './context';
import { markdownProvider } from './providers/markdown';
import { jsonProvider } from './providers/json';
import { codeProvider } from './providers/code';
import { mediaProvider } from './providers/media';
import { pdfProvider } from './providers/pdf';
import { csvProvider } from './providers/csv';
import { archiveProvider } from './providers/archive';

/** 内置 provider 列表（不含 HTML：H0 BLOCKED；T16/T17 已合入） */
export const BUILTIN_PROVIDERS: PreviewProvider[] = [
  markdownProvider,
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
