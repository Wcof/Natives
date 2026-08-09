/**
 * Error Classifier — structured error categorization with recovery actions.
 *
 * Pattern-matching classifier inspired by CodePilot's error-classifier.
 * Produces actionable, user-facing error messages with clickable recovery buttons.
 *
 * User-facing copy is locale-aware via i18n. Default locale is Chinese (zh),
 * matching the product default. Pass `{ locale }` when the active UI locale is known.
 */

import { t } from '@/i18n';

// ── Error categories ────────────────────────────────────────────

export type ErrorCategory =
  | 'PLUGIN_CRASH'
  | 'PLUGIN_TIMEOUT'
  | 'BRIDGE_PERMISSION_DENIED'
  | 'BRIDGE_INVALID_REQUEST'
  | 'MODULE_INSTALL_FAILED'
  | 'MODULE_NOT_FOUND'
  | 'TERMINAL_SPAWN_FAILED'
  | 'TERMINAL_CRASH'
  | 'DB_ERROR'
  | 'CONFIG_CORRUPTED'
  | 'NETWORK_ERROR'
  | 'AUTH_REJECTED'
  | 'AUTH_FORBIDDEN'
  | 'RATE_LIMITED'
  | 'IPC_TIMEOUT'
  | 'IPC_HANDLER_MISSING'
  | 'FILE_WRITE_FAILED'
  | 'FILE_READ_FAILED'
  | 'MUTATION_APPLIED_REFRESH_FAILED'
  | 'PROJECT_PATH_REQUIRED'
  | 'PROJECT_NOT_FOUND'
  | 'PROJECT_NOT_DIRECTORY'
  | 'PROJECT_REGISTER_FAILED'
  | 'UNKNOWN';

/** A concrete action the user can take to recover from an error */
export interface RecoveryAction {
  label: string;
  url?: string;
  action?: 'open_settings' | 'retry' | 'restart' | 'new_session';
}

export interface ClassifiedError {
  category: ErrorCategory;
  userMessage: string;
  actionHint: string;
  retryable: boolean;
  rawMessage: string;
  moduleId?: string;
  details?: string;
  recoveryActions?: RecoveryAction[];
}

// ── Classification context ──────────────────────────────────────

export interface ErrorContext {
  /** Optional when passed as the second arg; classifyError always injects the primary error. */
  error?: unknown;
  moduleId?: string;
  stderr?: string;
  /** Active UI locale. Defaults to Chinese. */
  locale?: string;
}

// ── Extraction Helpers ──────────────────────────────────────────

/**
 * Extract an error code from a structured daemon error or an Error instance.
 */
function extractErrorCode(error: unknown): string | undefined {
  if (error instanceof Error) return (error as NodeJS.ErrnoException).code;
  if (typeof error === 'object' && error !== null) {
    const obj = error as Record<string, unknown>;
    if (typeof obj.code === 'string') return obj.code;
  }
  return undefined;
}

/**
 * Extract a user-facing message from a structured daemon error.
 */
function extractErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null) {
    const obj = error as Record<string, unknown>;
    if (typeof obj.message === 'string' && obj.message) return obj.message;
    if (typeof obj.technical_message === 'string' && obj.technical_message) return obj.technical_message;
  }
  return String(error);
}

function resolveLocale(ctx: ErrorContext): string {
  return ctx.locale && ctx.locale.trim() ? ctx.locale : 'zh';
}

// ── Pattern definitions ─────────────────────────────────────────

interface ErrorPattern {
  category: ErrorCategory;
  patterns: Array<string | RegExp>;
  codes?: string[];
  userMessage: (ctx: ErrorContext) => string;
  actionHint: (ctx: ErrorContext) => string;
  retryable: boolean;
}

const ERROR_PATTERNS: ErrorPattern[] = [
  {
    category: 'MUTATION_APPLIED_REFRESH_FAILED',
    patterns: ['mutation applied on branch', 'mutation_succeeded_status_refresh_failed'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.mutationRefreshFailed'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintMutationRefresh'),
    retryable: true,
  },
  // ── Project errors (must be before TERMINAL_SPAWN_FAILED to avoid 'not found' clash) ──
  {
    category: 'PROJECT_PATH_REQUIRED',
    patterns: ['PROJECT_PATH_REQUIRED', 'project path is required'],
    codes: ['PROJECT_PATH_REQUIRED'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.projectPathRequired'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintProjectPath'),
    retryable: false,
  },
  {
    category: 'PROJECT_NOT_FOUND',
    patterns: ['PROJECT_NOT_FOUND', 'project directory not found', 'project path must exist'],
    codes: ['PROJECT_NOT_FOUND'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.projectNotFound'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintProjectNotFound'),
    retryable: false,
  },
  {
    category: 'PROJECT_NOT_DIRECTORY',
    patterns: ['PROJECT_NOT_DIRECTORY', 'project path must be a directory'],
    codes: ['PROJECT_NOT_DIRECTORY'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.projectNotDirectory'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintProjectNotDirectory'),
    retryable: false,
  },
  {
    category: 'PROJECT_REGISTER_FAILED',
    patterns: ['PROJECT_REGISTER_FAILED', 'project register', 'failed to register project'],
    codes: ['PROJECT_REGISTER_FAILED'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.projectRegisterFailed'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintProjectRegister'),
    retryable: true,
  },

  // ── Terminal spawn failed ──
  {
    category: 'TERMINAL_SPAWN_FAILED',
    patterns: ['ENOENT', 'spawn', 'pty', 'not found'],
    codes: ['ENOENT'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.terminalSpawnFailed'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintTerminalSpawn'),
    retryable: true,
  },

  // ── Terminal crash ──
  {
    category: 'TERMINAL_CRASH',
    patterns: [/terminal.*crash/i, /session.*exit/i, /pty.*error/i],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.terminalCrash'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintTerminalCrash'),
    retryable: true,
  },

  // ── Database errors ──
  {
    category: 'DB_ERROR',
    patterns: ['SQLITE', 'sqlite', 'database', 'db error', 'no such table', 'disk I/O'],
    codes: ['SQLITE_ERROR', 'SQLITE_CORRUPT', 'SQLITE_FULL'],
    userMessage: (ctx) =>
      t(resolveLocale(ctx), 'errors.dbErrorDetail', { detail: extractErrorMessage(ctx.error) }),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintDbError'),
    retryable: true,
  },

  // ── Module install failed ──
  {
    category: 'MODULE_INSTALL_FAILED',
    patterns: [/install.*fail/i, /manifest.*invalid/i, /module.*corrupt/i],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.installFailedSimple'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintInstallFailed'),
    retryable: true,
  },

  // ── Assistant daemon unavailable ──
  {
    category: 'NETWORK_ERROR',
    patterns: [/daemon.*not connected/i, /not connected to daemon/i, /failed to connect to daemon/i, /assistant rpc failed/i],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.assistantUnavailable'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintAssistantUnavailable'),
    retryable: true,
  },

  // ── Module not found ──
  {
    category: 'MODULE_NOT_FOUND',
    patterns: ['not found', 'missing', 'no such file', 'MODULE_NOT_FOUND'],
    userMessage: (ctx) =>
      ctx.moduleId
        ? t(resolveLocale(ctx), 'errors.moduleNotFoundNamed', { id: ctx.moduleId })
        : t(resolveLocale(ctx), 'errors.moduleNotFound'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintModuleNotFound'),
    retryable: false,
  },

  // ── Bridge permission denied ──
  {
    category: 'BRIDGE_PERMISSION_DENIED',
    patterns: ['permission', 'denied', 'forbidden', '403', 'unauthorized scope'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.permissionDenied'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintPermissionDenied'),
    retryable: false,
  },

  // ── Auth rejected (401) — must be before BRIDGE_INVALID_REQUEST ──
  {
    category: 'AUTH_REJECTED',
    patterns: ['401', 'Unauthorized', 'invalid_api_key', 'authentication failed', 'authentication_error'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.authRejected'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintAuthRejected'),
    retryable: false,
  },

  // ── Auth forbidden (403) ──
  {
    category: 'AUTH_FORBIDDEN',
    patterns: ['403', 'Forbidden', 'access denied', 'permission_error'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.authForbidden'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintAuthForbidden'),
    retryable: false,
  },

  // ── Bridge invalid request ──
  {
    category: 'BRIDGE_INVALID_REQUEST',
    patterns: ['malformed', 'bad request', '400', 'schema validation', 'invalid request'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.invalidRequest'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintInvalidRequest'),
    retryable: false,
  },

  // ── Rate limited (429) ──
  {
    category: 'RATE_LIMITED',
    patterns: ['429', 'rate limit', 'too many requests', 'overloaded', 'rate limited'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.rateLimited'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintRateLimited'),
    retryable: true,
  },

  // ── Network errors ──
  {
    category: 'NETWORK_ERROR',
    patterns: ['ECONNREFUSED', 'ECONNRESET', 'ETIMEDOUT', 'ENOTFOUND', 'fetch failed', 'network error'],
    codes: ['ECONNREFUSED', 'ECONNRESET', 'ETIMEDOUT', 'ENOTFOUND'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.networkError'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintNetworkError'),
    retryable: true,
  },

  // ── IPC timeout ──
  {
    category: 'IPC_TIMEOUT',
    patterns: [/ipc.*timeout/i, /invoke.*timeout/i, /handler.*timeout/i],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.requestTimedOut'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintRequestTimedOut'),
    retryable: true,
  },

  // ── IPC handler missing ──
  {
    category: 'IPC_HANDLER_MISSING',
    patterns: ['No handler', 'handler not registered', 'unknown channel'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.featureUnavailable'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintFeatureUnavailable'),
    retryable: false,
  },

  // ── File write failed ──
  {
    category: 'FILE_WRITE_FAILED',
    patterns: [/EACCES.*write/i, /EPERM.*write/i, 'ENOSPC', /write.*fail/i, /atomic.*fail/i],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.fileWriteFailed'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintFileWrite'),
    retryable: true,
  },

  // ── File read failed ──
  {
    category: 'FILE_READ_FAILED',
    patterns: [/EACCES.*read/i, /EPERM.*read/i, /read.*fail/i, /EISDIR/i],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.fileReadFailed'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintFileRead'),
    retryable: false,
  },

  // ── Config corrupted ──
  {
    category: 'CONFIG_CORRUPTED',
    patterns: [/config.*corrupt/i, /parse.*error/i, /JSON.*parse/i, /syntax.*error.*JSON/i],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.configCorrupted'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintConfigCorrupted'),
    retryable: false,
  },

  // ── Plugin crash ──
  {
    category: 'PLUGIN_CRASH',
    patterns: ['crash', 'segfault', 'SIGSEGV', 'core dumped'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.pluginCrash'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintPluginCrash'),
    retryable: true,
  },

  // ── Plugin timeout ──
  {
    category: 'PLUGIN_TIMEOUT',
    patterns: ['timeout', 'timed out', 'deadline exceeded'],
    userMessage: (ctx) => t(resolveLocale(ctx), 'errors.pluginTimeout'),
    actionHint: (ctx) => t(resolveLocale(ctx), 'errors.hintPluginTimeout'),
    retryable: true,
  },
];

// ── Recovery action builder ─────────────────────────────────────

function buildRecoveryActions(category: ErrorCategory, locale: string): RecoveryAction[] {
  switch (category) {
    case 'AUTH_REJECTED':
    case 'AUTH_FORBIDDEN':
      return [{ label: t(locale, 'errors.openSettings'), action: 'open_settings' }];
    case 'RATE_LIMITED':
    case 'NETWORK_ERROR':
    case 'IPC_TIMEOUT':
      return [{ label: t(locale, 'errors.retry'), action: 'retry' }];
    case 'TERMINAL_CRASH':
    case 'TERMINAL_SPAWN_FAILED':
      return [{ label: t(locale, 'errors.newSession'), action: 'new_session' }];
    case 'DB_ERROR':
    case 'CONFIG_CORRUPTED':
      return [{ label: t(locale, 'errors.restartApp'), action: 'restart' }];
    case 'MODULE_INSTALL_FAILED':
    case 'MODULE_NOT_FOUND':
      return [
        { label: t(locale, 'errors.retry'), action: 'retry' },
        { label: t(locale, 'errors.openSettings'), action: 'open_settings' },
      ];
    case 'PLUGIN_CRASH':
    case 'PLUGIN_TIMEOUT':
      return [{ label: t(locale, 'errors.retry'), action: 'retry' }];
    default:
      return [];
  }
}

// ── Classifier ──────────────────────────────────────────────────

/**
 * Classify an error into a structured error with user-facing message,
 * actionable hints, and recovery action buttons.
 *
 * Second argument may be a moduleId string (legacy) or full ErrorContext.
 * Locale defaults to Chinese when omitted.
 */
export function classifyError(error: unknown, moduleIdOrCtx?: string | ErrorContext): ClassifiedError {
  // Second arg may be a legacy moduleId string or a partial context ({ locale }, …).
  // Always prefer the primary `error` argument unless the context explicitly overrides it.
  const ctx: ErrorContext = typeof moduleIdOrCtx === 'string'
    ? { error, moduleId: moduleIdOrCtx }
    : { ...(moduleIdOrCtx ?? {}), error: moduleIdOrCtx?.error ?? error };

  const locale = resolveLocale(ctx);

  // Support structured errors: `{ code, message }`, `{ code, technical_message }`
  const errorCode = extractErrorCode(ctx.error);
  const rawMessage = extractErrorMessage(ctx.error);
  const stderrContent = ctx.stderr || '';
  const searchText = `${rawMessage}\n${stderrContent}`.toLowerCase();

  // Add the error code to the search text for pattern matching
  const searchTextWithCode = errorCode ? `${searchText}\n${errorCode.toLowerCase()}` : searchText;

  for (const pattern of ERROR_PATTERNS) {
    // Check error code first (most specific)
    if (pattern.codes && errorCode && pattern.codes.includes(errorCode)) {
      return buildResult(pattern, ctx, rawMessage, locale);
    }

    // Check patterns against combined text
    const matched = pattern.patterns.some(p => {
      if (typeof p === 'string') return searchTextWithCode.includes(p.toLowerCase());
      return p.test(searchTextWithCode);
    });

    if (matched) {
      return buildResult(pattern, ctx, rawMessage, locale);
    }
  }

  // Fallback with diagnostic code for unknown errors
  const diagnosticId = typeof crypto !== 'undefined' && crypto.randomUUID
    ? crypto.randomUUID().slice(0, 8)
    : Math.random().toString(36).slice(2, 10);
  return {
    category: 'UNKNOWN',
    userMessage: t(locale, 'errors.unknownWithDetail', {
      detail: rawMessage || t(locale, 'errors.unknown'),
      id: diagnosticId,
    }),
    actionHint: t(locale, 'errors.unknownWithDetail', {
      detail: rawMessage || t(locale, 'errors.unknown'),
      id: diagnosticId,
    }),
    retryable: false,
    rawMessage,
    moduleId: ctx.moduleId,
    recoveryActions: [],
  };
}

function buildResult(
  pattern: ErrorPattern,
  ctx: ErrorContext,
  rawMessage: string,
  locale: string,
): ClassifiedError {
  return {
    category: pattern.category,
    userMessage: pattern.userMessage(ctx),
    actionHint: pattern.actionHint(ctx),
    retryable: pattern.retryable,
    rawMessage,
    moduleId: ctx.moduleId,
    details: ctx.stderr || undefined,
    recoveryActions: buildRecoveryActions(pattern.category, locale),
  };
}

// ── Formatting helpers ──────────────────────────────────────────

/**
 * Format a ClassifiedError into a user-friendly string.
 */
export function formatClassifiedError(err: ClassifiedError, locale = 'zh'): string {
  let msg = err.userMessage;
  if (err.actionHint) {
    msg += `\n\n${t(locale, 'errors.suggestionLabel')}: ${err.actionHint}`;
  }
  if (err.details) {
    msg += `\n\n${t(locale, 'errors.detailsLabel')}: ${err.details}`;
  }
  return msg;
}

/**
 * Serialize a ClassifiedError to JSON for IPC transport.
 * Frontend can parse this to render structured error UI with recovery buttons.
 */
export function serializeClassifiedError(err: ClassifiedError): string {
  return JSON.stringify({
    category: err.category,
    userMessage: err.userMessage,
    actionHint: err.actionHint,
    retryable: err.retryable,
    moduleId: err.moduleId,
    details: err.details,
    rawMessage: err.rawMessage,
    recoveryActions: err.recoveryActions,
  });
}
