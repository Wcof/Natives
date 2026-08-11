/**
 * Diagnostics export（UX-12 · W8）— read-only Host diagnostics + limited
 * sanitized logs → local file.
 *
 * 白名单策略（redaction whitelist）：
 * 导出文件只允许包含 版本 / 协议 / instance / readiness / Run metadata（id、
 * status、error code、steps、model、时间戳）/ 有限脱敏日志 / redaction manifest。
 *
 * 永不包含：prompt、message、reasoning、delta、tool I/O、attachment、file 内容、
 * env、credential、DB、用户绝对路径。runtime-log 的条目在写入缓冲区时已脱敏
 * （sk-***、Bearer ***、home 目录 → ~），此处不再二次拼接原始日志。
 *
 * 复用现有底座：window.nativesAPI.dialog.saveFile（系统保存对话框）+
 * window.nativesAPI.fs.writeFileAtomic（原子写盘）。不新建第二套保存逻辑。
 */

import { getRecentLogs } from './runtime-log';

/** 导出中被排除的类别（manifest 的一部分，也是 UI 说明文案的来源）。 */
export const REDACTION_EXCLUDED_CATEGORIES = [
  'prompt',
  'message',
  'reasoning',
  'delta',
  'tool_io',
  'attachment',
  'file',
  'env',
  'credential',
  'db',
  'user_absolute_path',
] as const;

/** 导出中允许出现的类别（白名单）。 */
export const REDACTION_ALLOWED_CATEGORIES = [
  'version',
  'protocol',
  'instance',
  'readiness',
  'run_metadata',
  'run_digest',
  'error_code',
  'limited_redacted_logs',
  'redaction_manifest',
] as const;

/** Run metadata 白名单字段（绝不包含消息/工具输入输出内容）。 */
export interface DiagnosticsRunInfo {
  id: string;
  status?: string | null;
  errorCode?: string | null;
  providerId?: string | null;
  modelId?: string | null;
  maxSteps?: number | null;
  stepCount?: number | null;
  startedAt?: string | null;
  finishedAt?: string | null;
}

export interface DiagnosticsExportInput {
  locale: string;
  appVersion?: string | null;
  /** Host `execution_engine_get_diagnostics` 只读返回。 */
  engineDiagnostics?: Record<string, unknown> | null;
  /** 会话绑定项目（仅用于表示「本次导出来自该 surface」，不写绝对路径）。 */
  projectPath?: string | null;
  /** Run 元数据（白名单字段，可为 null —— 当前 surface 拿不到运行上下文）。 */
  run?: DiagnosticsRunInfo | null;
  /** 最多导出多少条已脱敏运行时日志（默认 60）。 */
  maxLogEntries?: number;
  /** 会话连接/就绪状态（可选，来自 assistant workspace store）。 */
  connection?: {
    connection?: string | null;
    reconnectAttempts?: number;
  } | null;
}

export interface DiagnosticsExportPayload {
  schemaVersion: number;
  exportedAt: string;
  app: { version: string | null; locale: string };
  instance: { authorityMode: string | null };
  readiness: {
    daemonReady: boolean | null;
    connection: string | null;
    reconnectAttempts: number | null;
  };
  protocol: {
    protocolVersion: string | null;
    eventStreamV1: boolean | null;
    streamTransport: string | null;
  };
  enginePolicy: Record<string, unknown>;
  run: DiagnosticsRunInfo | null;
  logs: Array<{ level: string; timestamp: string; message: string }>;
  redactionManifest: {
    excludedCategories: readonly string[];
    allowedCategories: readonly string[];
    note: string;
    storedLocallyOnly: boolean;
  };
}

function extractString(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function extractBool(value: unknown): boolean | null {
  return typeof value === 'boolean' ? value : null;
}

/** 纯函数：把各来源数据组装成白名单导出 JSON 文本（可单测）。 */
export function buildDiagnosticsExport(input: DiagnosticsExportInput): string {
  const engine = input.engineDiagnostics ?? {};
  const authorityMode = extractString(engine.authorityMode);
  const daemonReady = extractBool(engine.daemonReady);
  const protocolVersion =
    extractString(engine.protocolVersion) ?? extractString(engine.protocol);
  const eventStreamV1 = extractBool(engine.eventStreamV1);
  const streamTransport = extractString(engine.streamTransport);

  // 引擎策略摘要：仅保留白名单字段，过滤掉任何未知键（防御未知泄漏）。
  const policyKeys = [
    'maxSteps',
    'externalUnavailablePolicy',
    'defaultRuntime',
    'nativeDisabledTools',
  ] as const;
  const enginePolicy: Record<string, unknown> = {};
  for (const key of policyKeys) {
    if (key in engine) enginePolicy[key] = engine[key];
  }

  const maxLogEntries = Math.min(200, Math.max(0, input.maxLogEntries ?? 60));
  const logs = getRecentLogs()
    .slice(-maxLogEntries)
    .map((entry) => ({
      level: entry.level,
      timestamp: entry.timestamp,
      // 条目写入缓冲区时已脱敏（runtime-log scrubMessage）。
      message: entry.message.slice(0, 2000),
    }));

  const payload: DiagnosticsExportPayload = {
    schemaVersion: 1,
    exportedAt: new Date().toISOString(),
    app: { version: input.appVersion ?? null, locale: input.locale },
    instance: { authorityMode },
    readiness: {
      daemonReady,
      connection: extractString(input.connection?.connection ?? null),
      reconnectAttempts:
        typeof input.connection?.reconnectAttempts === 'number'
          ? input.connection.reconnectAttempts
          : null,
    },
    protocol: { protocolVersion, eventStreamV1, streamTransport },
    enginePolicy,
    run: input.run ?? null,
    logs,
    redactionManifest: {
      excludedCategories: REDACTION_EXCLUDED_CATEGORIES,
      allowedCategories: REDACTION_ALLOWED_CATEGORIES,
      note: 'Contains no prompts, messages, reasoning, deltas, tool I/O, attachments, file contents, environment variables, credentials, database contents, or user absolute paths. Stored locally only; never uploaded.',
      storedLocallyOnly: true,
    },
  };
  return JSON.stringify(payload, null, 2);
}

export type DiagnosticsExportResult =
  | { ok: true; path: string }
  | { ok: true; cancelled: true }
  | { ok: false; error: string };

/**
 * 编排导出：系统保存对话框 → 原子写盘。复用现有 save dialog / writeFileAtomic。
 * 非 Tauri 环境（浏览器 dev）返回 ok:false，调用方展示诚实错误态。
 */
export async function saveDiagnosticsExport(
  input: DiagnosticsExportInput,
): Promise<DiagnosticsExportResult> {
  const api = window.nativesAPI;
  if (!api?.dialog?.saveFile || !api?.fs?.writeFileAtomic) {
    return { ok: false, error: 'diagnostics-export unavailable (Tauri IPC required)' };
  }
  const targetPath = await api.dialog.saveFile().catch(() => null);
  if (!targetPath) return { ok: true, cancelled: true };

  const json = buildDiagnosticsExport(input);
  try {
    await api.fs.writeFileAtomic(targetPath, json, undefined);
    return { ok: true, path: targetPath };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** 默认导出文件名（保存对话框由用户决定位置；此函数仅供显示/默认值参考）。 */
export function diagnosticsExportBasename(date: Date = new Date()): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `natives-diagnostics-${y}${m}${d}.json`;
}
