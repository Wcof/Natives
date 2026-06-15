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
  | 'UNKNOWN';

export interface ClassifiedError {
  category: ErrorCategory;
  userMessage: string;
  actionHint: string;
  retryable: boolean;
  rawMessage?: string;
  moduleId?: string;
}

const ERROR_META: Record<ErrorCategory, { userMessage: string; actionHint: string; retryable: boolean }> = {
  PLUGIN_CRASH: {
    userMessage: 'A plugin has crashed',
    actionHint: 'Try reloading the plugin. If the problem persists, contact the developer.',
    retryable: true,
  },
  PLUGIN_TIMEOUT: {
    userMessage: 'A plugin is not responding',
    actionHint: 'The plugin took too long to load. Try reloading.',
    retryable: true,
  },
  BRIDGE_PERMISSION_DENIED: {
    userMessage: 'Permission denied',
    actionHint: 'This plugin does not have the required permission. Check plugin settings.',
    retryable: false,
  },
  BRIDGE_INVALID_REQUEST: {
    userMessage: 'Invalid request',
    actionHint: 'The plugin sent a malformed request. Contact the developer.',
    retryable: false,
  },
  MODULE_INSTALL_FAILED: {
    userMessage: 'Module installation failed',
    actionHint: 'Check that the module has a valid manifest.json and try again.',
    retryable: true,
  },
  MODULE_NOT_FOUND: {
    userMessage: 'Module not found',
    actionHint: 'The module may have been moved or deleted. Try reinstalling.',
    retryable: false,
  },
  TERMINAL_SPAWN_FAILED: {
    userMessage: 'Failed to start terminal',
    actionHint: 'Check that your shell (zsh/bash) is available and try again.',
    retryable: true,
  },
  TERMINAL_CRASH: {
    userMessage: 'Terminal session ended unexpectedly',
    actionHint: 'The terminal process exited. You can start a new session.',
    retryable: true,
  },
  DB_ERROR: {
    userMessage: 'Database error',
    actionHint: 'An internal database error occurred. Try restarting the application.',
    retryable: true,
  },
  CONFIG_CORRUPTED: {
    userMessage: 'Configuration file corrupted',
    actionHint: 'The configuration file could not be read. Default settings will be used.',
    retryable: false,
  },
  NETWORK_ERROR: {
    userMessage: 'Network error',
    actionHint: 'Check your internet connection and try again.',
    retryable: true,
  },
  UNKNOWN: {
    userMessage: 'An unexpected error occurred',
    actionHint: 'Try restarting the application. If the problem persists, report the issue.',
    retryable: false,
  },
};

function classifyByMessage(raw: string): ErrorCategory {
  const lower = raw.toLowerCase();
  if (lower.includes('install fail') || (lower.includes('install') && lower.includes('invalid'))) return 'MODULE_INSTALL_FAILED';
  if (lower.includes('sqlite') || lower.includes('database') || lower.includes('db error')) return 'DB_ERROR';
  if (lower.includes('terminal') && lower.includes('crash')) return 'TERMINAL_CRASH';
  if (lower.includes('spawn') || lower.includes('pty')) {
    if (lower.includes('fail') || lower.includes('error')) return 'TERMINAL_SPAWN_FAILED';
    return 'TERMINAL_CRASH';
  }
  if (lower.includes('timeout') || lower.includes('timed out')) return 'PLUGIN_TIMEOUT';
  if (lower.includes('permission') || lower.includes('denied') || lower.includes('forbidden')) return 'BRIDGE_PERMISSION_DENIED';
  if (lower.includes('invalid') || lower.includes('malformed') || lower.includes('bad request')) return 'BRIDGE_INVALID_REQUEST';
  if (lower.includes('not found') || lower.includes('missing') || lower.includes('no such')) return 'MODULE_NOT_FOUND';
  if ((lower.includes('config') && (lower.includes('corrupt') || lower.includes('parse'))) || lower.includes('parse error')) return 'CONFIG_CORRUPTED';
  if (lower.includes('network') || lower.includes('econnrefused') || lower.includes('enotfound') || lower.includes('fetch')) return 'NETWORK_ERROR';
  if (lower.includes('crash') || lower.includes('segfault')) return 'PLUGIN_CRASH';
  return 'UNKNOWN';
}

export function classifyError(error: unknown, moduleId?: string): ClassifiedError {
  const rawMessage = error instanceof Error ? error.message : typeof error === 'object' && error !== null ? JSON.stringify(error) : String(error);
  const category = classifyByMessage(rawMessage);
  const meta = ERROR_META[category];

  return {
    category,
    userMessage: meta.userMessage,
    actionHint: meta.actionHint,
    retryable: meta.retryable,
    rawMessage,
    moduleId,
  };
}
