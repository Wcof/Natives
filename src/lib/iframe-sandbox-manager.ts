// ── IframeSandboxManager (P0-3: renderer-process only, no Node.js crypto) ──

export interface IframeRecord {
  moduleId: string;
  contentWindow: Window | null;
  token: string;
  sessionStart: number;
  lastHeartbeat: number;
}

export class IframeSandboxManager {
  private iframes = new Map<string, IframeRecord>();
  private sourceMap = new Map<string, Window | null>();

  register(moduleId: string, contentWindow: Window | null): { token: string } {
    // Token generation is handled by main process; renderer stores reference
    const record: IframeRecord = {
      moduleId,
      contentWindow,
      token: '',
      sessionStart: Date.now(),
      lastHeartbeat: Date.now(),
    };
    this.iframes.set(moduleId, record);
    this.sourceMap.set(moduleId, contentWindow);
    return { token: '' };
  }

  unregister(moduleId: string): void {
    this.iframes.delete(moduleId);
    this.sourceMap.delete(moduleId);
  }

  verifyMessageSource(moduleId: string, source: Window | null): boolean {
    return this.sourceMap.get(moduleId) === source;
  }

  getToken(moduleId: string): string | undefined {
    return this.iframes.get(moduleId)?.token;
  }

  updateHeartbeat(moduleId: string): void {
    const record = this.iframes.get(moduleId);
    if (record) record.lastHeartbeat = Date.now();
  }

  getTimeoutCount(moduleId: string, timeoutMs: number): number {
    const record = this.iframes.get(moduleId);
    if (!record) return 0;
    return Math.floor((Date.now() - record.lastHeartbeat) / timeoutMs);
  }
}
