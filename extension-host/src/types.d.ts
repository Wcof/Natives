// Ambient type declarations for the Node built-in test runner.
// The extension host test files use node:test instead of the tsx:test
// virtual module, because tsx's `tsx:` protocol is not supported by the
// Node 22 default ESM loader (ERR_UNSUPPORTED_ESM_URL_SCHEME).
declare module 'node:test' {
  export function describe(name: string, fn: () => void | Promise<void>): void;
  export function it(name: string, fn: () => void | Promise<void>): void;
}

declare module 'node:assert/strict' {
  const assert: {
    equal(actual: unknown, expected: unknown, message?: string): void;
    ok(value: unknown, message?: string): void;
    fail(message?: string): void;
  };
  export default assert;
}
