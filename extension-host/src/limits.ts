// ─── Extension Limits ────────────────────────────────────
//
// Resource limits for extensions: timeout, output size, and memory.

export interface LimitsConfig {
  timeoutMs: number;
  maxOutputBytes: number;
  maxMemoryMb: number;
}

export class Limits {
  private config: LimitsConfig;

  constructor(config: Partial<LimitsConfig> = {}) {
    this.config = {
      timeoutMs: config.timeoutMs ?? 30_000,
      maxOutputBytes: config.maxOutputBytes ?? 1_048_576,
      maxMemoryMb: config.maxMemoryMb ?? 128,
    };
  }

  get timeoutMs(): number {
    return this.config.timeoutMs;
  }

  get maxOutputBytes(): number {
    return this.config.maxOutputBytes;
  }

  get maxMemoryMb(): number {
    return this.config.maxMemoryMb;
  }

  /**
   * Check if the output size is within limits.
   */
  checkOutputSize(size: number): boolean {
    return size <= this.config.maxOutputBytes;
  }

  /**
   * Create a timeout promise that rejects after the configured timeout.
   */
  createTimeout<T>(promise: Promise<T>): Promise<T> {
    return Promise.race([
      promise,
      new Promise<T>((_, reject) =>
        setTimeout(() => reject(new Error(`Extension timed out after ${this.config.timeoutMs}ms`)), this.config.timeoutMs)
      ),
    ]);
  }
}