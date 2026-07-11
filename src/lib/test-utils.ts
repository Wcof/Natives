// ─── Test Utilities ──────────────────────────────────────
//
// Minimal test utilities for use with tsx test runner.

export function describe(_name: string, fn: () => void): void {
  fn();
}

export function it(_name: string, fn: () => void | Promise<void>): void {
  const result = fn();
  if (result instanceof Promise) {
    // Async test - handled by the test runner
  }
}

export const assert = {
  equal: (actual: unknown, expected: unknown, message?: string) => {
    if (actual !== expected) {
      throw new Error(message || `Expected ${expected}, got ${actual}`);
    }
  },
  ok: (value: unknown, message?: string) => {
    if (!value) {
      throw new Error(message || `Expected truthy value, got ${value}`);
    }
  },
  fail: (message?: string) => {
    throw new Error(message || 'Test failed');
  },
};

// ─── Mock Setup ──────────────────────────────────────────

export const mock = {
  setup: (_modulePath: string, _exports: Record<string, unknown>) => {
    // Mock setup is a no-op for now
  },
  create: () => {
    const handlers = new Map<string, (...args: unknown[]) => unknown>();

    const fn = (...args: unknown[]) => {
      const key = args[0] as string;
      const handler = handlers.get(key);
      if (handler) {
        return handler(...args);
      }
      throw new Error(`No mock handler for: ${key}`);
    };

    fn.withArgs = (key: string) => {
      const wrapper = {
        resolves: (value: unknown) => {
          handlers.set(key, () => Promise.resolve(value));
          return wrapper;
        },
        rejects: (error: unknown) => {
          handlers.set(key, () => Promise.reject(error));
          return wrapper;
        },
      };
      return wrapper;
    };

    return fn;
  },
};