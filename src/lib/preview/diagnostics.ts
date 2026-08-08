// Preview 诊断（T10 · C0 frozen）
//
// 轻量 timing/error 记录：provider 选择、耗时、结果类型，供 Surface 与调试使用。
// 有界环形缓冲（R-P9 缓存有界），不保存跨 Surface 的请求内容。

import type { PreviewRequest } from './contracts';

export interface PreviewDiagnosticRecord {
  surface: string;
  sourceType: 'file' | 'memory';
  pathOrName: string;
  providerId: string;
  elapsedMs: number;
  outcome: string;
  at: number;
}

export class PreviewDiagnostics {
  private buffer: PreviewDiagnosticRecord[] = [];
  constructor(private readonly limit = 64) {}

  record(
    request: PreviewRequest,
    providerId: string,
    elapsedMs: number,
    outcome: string | undefined,
  ): void {
    const source = request.source;
    this.buffer.push({
      surface: request.surface,
      sourceType: source.type,
      pathOrName: source.type === 'file' ? source.path : source.name,
      providerId,
      elapsedMs,
      outcome: outcome ?? 'ok',
      at: Date.now(),
    });
    if (this.buffer.length > this.limit) {
      this.buffer.splice(0, this.buffer.length - this.limit);
    }
  }

  snapshot(): readonly PreviewDiagnosticRecord[] {
    return this.buffer;
  }

  clear(): void {
    this.buffer = [];
  }
}
