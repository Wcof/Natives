/**
 * Error Classifier — structured error categorization with recovery actions.
 *
 * Pattern-matching classifier inspired by CodePilot's error-classifier.
 * Produces actionable, user-facing error messages with clickable recovery buttons.
 */

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
  error: unknown;
  moduleId?: string;
  stderr?: string;
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
    userMessage: () => 'The branch changed, but its latest status could not be loaded',
    actionHint: () => 'Refresh the Git status. Do not repeat the branch switch.',
    retryable: true,
  },
  // ── Project errors (must be before TERMINAL_SPAWN_FAILED to avoid 'not found' clash) ──
  {
    category: 'PROJECT_PATH_REQUIRED',
    patterns: ['PROJECT_PATH_REQUIRED', 'project path is required'],
    codes: ['PROJECT_PATH_REQUIRED'],
    userMessage: () => 'Project path is required',
    actionHint: () => 'Please provide a valid project folder path.',
    retryable: false,
  },
  {
    category: 'PROJECT_NOT_FOUND',
    patterns: ['PROJECT_NOT_FOUND', 'project directory not found', 'project path must exist'],
    codes: ['PROJECT_NOT_FOUND'],
    userMessage: () => 'Project directory not found',
    actionHint: () => 'The project folder may have been moved or deleted. Please select a valid folder.',
    retryable: false,
  },
  {
    category: 'PROJECT_NOT_DIRECTORY',
    patterns: ['PROJECT_NOT_DIRECTORY', 'project path must be a directory'],
    codes: ['PROJECT_NOT_DIRECTORY'],
    userMessage: () => 'Selected path is not a directory',
    actionHint: () => 'Please select a folder, not a file.',
    retryable: false,
  },
  {
    category: 'PROJECT_REGISTER_FAILED',
    patterns: ['PROJECT_REGISTER_FAILED', 'project register', 'failed to register project'],
    codes: ['PROJECT_REGISTER_FAILED'],
    userMessage: () => 'Failed to register project',
    actionHint: () => 'The project could not be registered. Check permissions and try again.',
    retryable: true,
  },

  // ── Terminal spawn failed ──
  {
    category: 'TERMINAL_SPAWN_FAILED',
    patterns: ['ENOENT', 'spawn', 'pty', 'not found'],
    codes: ['ENOENT'],
    userMessage: () => 'Failed to start terminal',
    actionHint: () => 'Check that your shell (zsh/bash) is available and try again.',
    retryable: true,
  },

  // ── Terminal crash ──
  {
    category: 'TERMINAL_CRASH',
    patterns: [/terminal.*crash/i, /session.*exit/i, /pty.*error/i],
    userMessage: () => 'Terminal session ended unexpectedly',
    actionHint: () => 'The terminal process exited. You can start a new session.',
    retryable: true,
  },

  // ── Database errors ──
  {
    category: 'DB_ERROR',
    patterns: ['SQLITE', 'sqlite', 'database', 'db error', 'no such table', 'disk I/O'],
    codes: ['SQLITE_ERROR', 'SQLITE_CORRUPT', 'SQLITE_FULL'],
    userMessage: (ctx) => `Database error: ${extractErrorMessage(ctx.error)}`,
    actionHint: () => 'An internal database error occurred. Try restarting the application.',
    retryable: true,
  },

  // ── Module install failed ──
  {
    category: 'MODULE_INSTALL_FAILED',
    patterns: [/install.*fail/i, /manifest.*invalid/i, /module.*corrupt/i],
    userMessage: () => 'Module installation failed',
    actionHint: () => 'Check that the module has a valid manifest.json and try again.',
    retryable: true,
  },

  // ── Assistant daemon unavailable ──
  {
    category: 'NETWORK_ERROR',
    patterns: [/daemon.*not connected/i, /not connected to daemon/i, /failed to connect to daemon/i, /assistant rpc failed/i],
    userMessage: () => 'Assistant service is unavailable',
    actionHint: () => 'Restart the application and try again.',
    retryable: true,
  },

  // ── Module not found ──
  {
    category: 'MODULE_NOT_FOUND',
    patterns: ['not found', 'missing', 'no such file', 'MODULE_NOT_FOUND'],
    userMessage: (ctx) => ctx.moduleId ? `Module not found: ${ctx.moduleId}` : 'Module not found',
    actionHint: () => 'The module may have been moved or deleted. Try reinstalling.',
    retryable: false,
  },

  // ── Bridge permission denied ──
  {
    category: 'BRIDGE_PERMISSION_DENIED',
    patterns: ['permission', 'denied', 'forbidden', '403', 'unauthorized scope'],
    userMessage: () => 'Permission denied',
    actionHint: () => 'This plugin does not have the required permission. Check plugin settings.',
    retryable: false,
  },

  // ── Auth rejected (401) — must be before BRIDGE_INVALID_REQUEST ──
  {
    category: 'AUTH_REJECTED',
    patterns: ['401', 'Unauthorized', 'invalid_api_key', 'authentication failed', 'authentication_error'],
    userMessage: () => 'Authentication failed',
    actionHint: () => 'Verify your API key is correct and has not expired.',
    retryable: false,
  },

  // ── Auth forbidden (403) ──
  {
    category: 'AUTH_FORBIDDEN',
    patterns: ['403', 'Forbidden', 'access denied', 'permission_error'],
    userMessage: () => 'Access denied',
    actionHint: () => 'Your API key may lack permissions for this operation.',
    retryable: false,
  },

  // ── Bridge invalid request ──
  {
    category: 'BRIDGE_INVALID_REQUEST',
    patterns: ['malformed', 'bad request', '400', 'schema validation', 'invalid request'],
    userMessage: () => 'Invalid request',
    actionHint: () => 'The plugin sent a malformed request. Contact the developer.',
    retryable: false,
  },

  // ── Rate limited (429) ──
  {
    category: 'RATE_LIMITED',
    patterns: ['429', 'rate limit', 'too many requests', 'overloaded'],
    userMessage: () => 'Rate limit exceeded',
    actionHint: () => 'Wait a moment before retrying. If this persists, consider upgrading your plan.',
    retryable: true,
  },

  // ── Network errors ──
  {
    category: 'NETWORK_ERROR',
    patterns: ['ECONNREFUSED', 'ECONNRESET', 'ETIMEDOUT', 'ENOTFOUND', 'fetch failed', 'network error'],
    codes: ['ECONNREFUSED', 'ECONNRESET', 'ETIMEDOUT', 'ENOTFOUND'],
    userMessage: () => 'Network error',
    actionHint: () => 'Check your internet connection and try again.',
    retryable: true,
  },

  // ── IPC timeout ──
  {
    category: 'IPC_TIMEOUT',
    patterns: [/ipc.*timeout/i, /invoke.*timeout/i, /handler.*timeout/i],
    userMessage: () => 'Request timed out',
    actionHint: () => 'The operation took too long. The main process may be busy. Try again.',
    retryable: true,
  },

  // ── IPC handler missing ──
  {
    category: 'IPC_HANDLER_MISSING',
    patterns: ['No handler', 'handler not registered', 'unknown channel'],
    userMessage: () => 'Feature not available',
    actionHint: () => 'This feature may not be loaded yet. Try restarting the application.',
    retryable: false,
  },

  // ── File write failed ──
  {
    category: 'FILE_WRITE_FAILED',
    patterns: [/EACCES.*write/i, /EPERM.*write/i, 'ENOSPC', /write.*fail/i, /atomic.*fail/i],
    userMessage: () => 'Failed to save file',
    actionHint: () => 'Check file permissions and available disk space.',
    retryable: true,
  },

  // ── File read failed ──
  {
    category: 'FILE_READ_FAILED',
    patterns: [/EACCES.*read/i, /EPERM.*read/i, /read.*fail/i, /EISDIR/i],
    userMessage: () => 'Failed to read file',
    actionHint: () => 'Check that the file exists and you have read permissions.',
    retryable: false,
  },

  // ── Config corrupted ──
  {
    category: 'CONFIG_CORRUPTED',
    patterns: [/config.*corrupt/i, /parse.*error/i, /JSON.*parse/i, /syntax.*error.*JSON/i],
    userMessage: () => 'Configuration file corrupted',
    actionHint: () => 'The configuration file could not be read. Default settings will be used.',
    retryable: false,
  },

  // ── Plugin crash ──
  {
    category: 'PLUGIN_CRASH',
    patterns: ['crash', 'segfault', 'SIGSEGV', 'core dumped'],
    userMessage: () => 'A plugin has crashed',
    actionHint: () => 'Try reloading the plugin. If the problem persists, contact the developer.',
    retryable: true,
  },

  // ── Plugin timeout ──
  {
    category: 'PLUGIN_TIMEOUT',
    patterns: ['timeout', 'timed out', 'deadline exceeded'],
    userMessage: () => 'A plugin is not responding',
    actionHint: () => 'The plugin took too long to load. Try reloading.',
    retryable: true,
  },
];

// ── Recovery action builder ─────────────────────────────────────

function buildRecoveryActions(category: ErrorCategory): RecoveryAction[] {
  switch (category) {
    case 'AUTH_REJECTED':
    case 'AUTH_FORBIDDEN':
      return [
        { label: 'Open Settings', action: 'open_settings' },
      ];
    case 'RATE_LIMITED':
      return [
        { label: 'Retry', action: 'retry' },
      ];
    case 'NETWORK_ERROR':
    case 'IPC_TIMEOUT':
      return [
        { label: 'Retry', action: 'retry' },
      ];
    case 'TERMINAL_CRASH':
    case 'TERMINAL_SPAWN_FAILED':
      return [
        { label: 'New Session', action: 'new_session' },
      ];
    case 'DB_ERROR':
    case 'CONFIG_CORRUPTED':
      return [
        { label: 'Restart App', action: 'restart' },
      ];
    case 'MODULE_INSTALL_FAILED':
    case 'MODULE_NOT_FOUND':
      return [
        { label: 'Retry', action: 'retry' },
        { label: 'Open Settings', action: 'open_settings' },
      ];
    case 'PLUGIN_CRASH':
    case 'PLUGIN_TIMEOUT':
      return [
        { label: 'Retry', action: 'retry' },
      ];
    default:
      return [];
  }
}

// ── Classifier ──────────────────────────────────────────────────

/**
 * Classify an error into a structured error with user-facing message,
 * actionable hints, and recovery action buttons.
 */


export function classifyError(error: unknown, moduleIdOrCtx?: string | ErrorContext): ClassifiedError {
  const ctx: ErrorContext = typeof moduleIdOrCtx === 'string'
    ? { error, moduleId: moduleIdOrCtx }
    : moduleIdOrCtx || { error };

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
      return buildResult(pattern, ctx, rawMessage);
    }

    // Check patterns against combined text
    const matched = pattern.patterns.some(p => {
      if (typeof p === 'string') return searchTextWithCode.includes(p.toLowerCase());
      return p.test(searchTextWithCode);
    });

    if (matched) {
      return buildResult(pattern, ctx, rawMessage);
    }
  }

  // Fallback with diagnostic code for unknown errors
  const diagnosticId = typeof crypto !== 'undefined' && crypto.randomUUID
    ? crypto.randomUUID().slice(0, 8)
    : Math.random().toString(36).slice(2, 10);
  return {
    category: 'UNKNOWN',
    userMessage: `操作未完成：${rawMessage || '未知错误'}。诊断编号：${diagnosticId}。`,
    actionHint: `Operation failed: ${rawMessage || 'Unknown error'}. Diagnostic ID: ${diagnosticId}.`,
    retryable: false,
    rawMessage,
    moduleId: ctx.moduleId,
    recoveryActions: [],
  };
}

function buildResult(pattern: ErrorPattern, ctx: ErrorContext, rawMessage: string): ClassifiedError {
  return {
    category: pattern.category,
    userMessage: pattern.userMessage(ctx),
    actionHint: pattern.actionHint(ctx),
    retryable: pattern.retryable,
    rawMessage,
    moduleId: ctx.moduleId,
    details: ctx.stderr || undefined,
    recoveryActions: buildRecoveryActions(pattern.category),
  };
}

// ── Formatting helpers ──────────────────────────────────────────

/**
 * Format a ClassifiedError into a user-friendly string.
 */
export function formatClassifiedError(err: ClassifiedError): string {
  let msg = err.userMessage;
  if (err.actionHint) msg += `\n\nWhat to do: ${err.actionHint}`;
  if (err.details) msg += `\n\nDetails: ${err.details}`;
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
