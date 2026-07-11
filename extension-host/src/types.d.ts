// Type declarations for tsx:test module
declare module 'tsx:test' {
  export function describe(name: string, fn: () => void): void;
  export function it(name: string, fn: () => void | Promise<void>): void;
  export const assert: {
    equal: (actual: unknown, expected: unknown, message?: string) => void;
    ok: (value: unknown, message?: string) => void;
    fail: (message?: string) => void;
  };
}