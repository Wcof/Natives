// T19 · Preview composition（唯一组装者）
//
// 本文件是 builtin registry / default PreviewContext 的唯一组装点。
// Leaf（T12–T17, T20）只导出 provider/renderer；T19 把它们注册进统一 registry，
// 并把业务 prepare 逻辑留在 provider，不在此重写。
// HTML lane remains disabled while docs/h0-decision.md is BLOCKED. HTML text
// deliberately falls through to the bounded code provider.

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

/** 内置 provider 列表；HTML 在 H1 headed evidence 通过前不注册。 */
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
