// ─── Extension RPC ───────────────────────────────────────
//
// RPC interface for the extension host. All calls are routed
// through the Rust capability gateway for policy enforcement.

export interface RPCRequest {
  method: string;
  params: unknown;
  requestId: string;
}

export interface RPCResponse {
  success: boolean;
  data?: unknown;
  error?: string;
  requestId: string;
}

export class ExtensionRPC {
  private requestId = 0;
  private pending = new Map<string, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();

  /**
   * Call a method through the RPC interface.
   * In production, this would route through the Rust capability gateway.
   */
  async call(method: string, params: unknown = {}): Promise<unknown> {
    const requestId = `ext-${++this.requestId}`;

    // Simulate RPC call (in production, this goes through the gateway)
    return new Promise((resolve, reject) => {
      this.pending.set(requestId, { resolve, reject });

      // Simulate response
      setTimeout(() => {
        this.pending.delete(requestId);
        resolve({ status: 'ok', method, params });
      }, 10);
    });
  }

  /**
   * Handle an incoming RPC response.
   */
  handleResponse(requestId: string, response: RPCResponse): void {
    const pending = this.pending.get(requestId);
    if (!pending) return;

    this.pending.delete(requestId);
    if (response.success) {
      pending.resolve(response.data);
    } else {
      pending.reject(new Error(response.error || 'RPC call failed'));
    }
  }
}