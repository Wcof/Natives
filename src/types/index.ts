// Global type declarations for Natives2 — Tauri IPC types.
// The authoritative interface definition lives in src/lib/tauri-adapter.ts.
// This file augments Window.nativesAPI so the rest of the codebase can type-check.

export {};

declare global {
  interface Window {
    __nativesHttpPort?: never; // Removed — use getHttpPort() helper instead
    nativesAPI?: import('../lib/tauri-adapter').NativesAPI;
    __nativesCmd?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  }
}
