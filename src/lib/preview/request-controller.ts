// Preview Request Controller（T10 · C0 frozen）
//
// stale/cancel 权威属于「每个 Surface 自己的 controller」，不属于全局 service。
// Files / Assistant / Artifact / Follow 各自持有实例；一个 Surface 的新请求
// 不会让另一个 Surface 的进行中请求被误判 stale/cancel（R16）。

export interface PreviewRequestToken {
  generation: number;
  signal: AbortSignal;
}

export class PreviewRequestController {
  private generation = 0;
  private abort: AbortController | null = null;

  /** 发起新请求：取消本 Surface 旧请求并返回新 token */
  next(): PreviewRequestToken {
    this.abort?.abort();
    this.abort = new AbortController();
    return { generation: ++this.generation, signal: this.abort.signal };
  }

  /** 只有最新一次请求的 generation 是当前有效的 */
  isCurrent(generation: number): boolean {
    return generation === this.generation;
  }

  /** 取消本 Surface 当前请求（不触碰其他 Surface） */
  cancel(): void {
    this.abort?.abort();
    this.generation++;
  }

  get currentGeneration(): number {
    return this.generation;
  }
}
