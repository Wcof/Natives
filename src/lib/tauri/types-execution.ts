/**
 * tauri/types-execution — Execution Engine V2 domain 共享类型（ARCH-002 split）
 *
 * 执行引擎设置 / 快照 / 运行时描述（E2-01）wire 类型声明于此；
 * execution-engine facade（./execution-engine.ts）与业务组件从这里取类型。
 */

// ── Execution Engine V2 (E2-01) ───────────────────────────────────────────
// Backend-derived truth: the snapshot is built by the Host from real runtime
// discovery + the daemon capability handshake (SETTINGS-002). Capability map
// keys/values mirror the daemon's advertised matrix; statuses are real probe
// output (ready | degraded | blocked | disabled | not_installed).

/** Runtime descriptor for the settings snapshot (Host-projected, real data). */
export interface RuntimeDescriptor {
  id: string;
  displayName: string;
  status: string;
  version?: string | null;
  authority: string;
  reasonCode: string;
  reason: string;
  capabilities: Record<string, string>;
  controllable: string[];
}

export interface ResolvedDefaultRuntime {
  runtimeId: string;
  source: string;
  fallbackUsed: boolean;
  reasonCode: string;
  reason: string;
}

/** ExecutionEngineSettingsV2 wire shape (camelCase). */
export interface ExecutionEngineSettings {
  schemaVersion: number;
  revision: number;
  defaultRuntime: string;
  externalUnavailablePolicy: string;
  native: { maxSteps: number; disabledTools: string[] };
  claudeCli: { enabled: boolean };
  codexCli: { enabled: boolean };
  diagnostics: { performanceTelemetry: boolean };
}

export interface ExecutionEngineSnapshot {
  settings: ExecutionEngineSettings;
  runtimes: RuntimeDescriptor[];
  resolvedDefault: ResolvedDefaultRuntime;
  defaultProvider?: unknown;
  diagnosticsSummary: Record<string, unknown>;
}
