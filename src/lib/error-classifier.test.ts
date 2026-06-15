import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { classifyError } from './error-classifier';

describe('ErrorClassifier', () => {
  it('should classify plugin crash', () => {
    const err = classifyError(new Error('Plugin crashed: segfault at 0x0'));
    assert.equal(err.category, 'PLUGIN_CRASH');
    assert.equal(err.retryable, true);
    assert.ok(err.userMessage);
    assert.ok(err.actionHint);
    assert.ok(err.recoveryActions && err.recoveryActions.length > 0);
  });

  it('should classify timeout', () => {
    const err = classifyError('Plugin timed out after 30s');
    assert.equal(err.category, 'PLUGIN_TIMEOUT');
    assert.equal(err.retryable, true);
  });

  it('should classify permission denied', () => {
    const err = classifyError('Permission denied: db:write');
    assert.equal(err.category, 'BRIDGE_PERMISSION_DENIED');
    assert.equal(err.retryable, false);
  });

  it('should classify module install failed', () => {
    const err = classifyError('Module install failed: invalid manifest');
    assert.equal(err.category, 'MODULE_INSTALL_FAILED');
    assert.equal(err.retryable, true);
  });

  it('should classify terminal spawn failed', () => {
    const err = classifyError('Failed to spawn PTY: ENOENT');
    assert.equal(err.category, 'TERMINAL_SPAWN_FAILED');
    assert.equal(err.retryable, true);
  });

  it('should classify database error', () => {
    const err = classifyError('SQLITE_ERROR: no such table');
    assert.equal(err.category, 'DB_ERROR');
  });

  it('should classify network error', () => {
    const err = classifyError('ECONNREFUSED connection refused');
    assert.equal(err.category, 'NETWORK_ERROR');
    assert.equal(err.retryable, true);
  });

  it('should classify unknown errors', () => {
    const err = classifyError('Something completely unexpected');
    assert.equal(err.category, 'UNKNOWN');
    assert.equal(err.retryable, false);
  });

  it('should include moduleId when provided', () => {
    const err = classifyError(new Error('crash'), 'com.example.app');
    assert.equal(err.moduleId, 'com.example.app');
  });

  it('should handle non-Error objects', () => {
    const err = classifyError({ code: 500, message: 'timeout' });
    assert.equal(err.category, 'PLUGIN_TIMEOUT');
  });

  // ── New pattern-matching tests ──

  it('should classify auth rejected (401)', () => {
    const err = classifyError('401 Unauthorized: invalid_api_key');
    assert.equal(err.category, 'AUTH_REJECTED');
    assert.equal(err.retryable, false);
    assert.ok(err.recoveryActions && err.recoveryActions.length > 0);
  });

  it('should classify rate limited (429)', () => {
    const err = classifyError('429 Too Many Requests');
    assert.equal(err.category, 'RATE_LIMITED');
    assert.equal(err.retryable, true);
  });

  it('should classify IPC timeout', () => {
    const err = classifyError('IPC invoke timeout: module:scan');
    assert.equal(err.category, 'IPC_TIMEOUT');
    assert.equal(err.retryable, true);
  });

  it('should classify file write failed', () => {
    const err = classifyError('ENOSPC: no space left on device');
    assert.equal(err.category, 'FILE_WRITE_FAILED');
    assert.equal(err.retryable, true);
  });

  it('should classify config corrupted', () => {
    const err = classifyError('JSON parse error in config file');
    assert.equal(err.category, 'CONFIG_CORRUPTED');
    assert.equal(err.retryable, false);
  });

  it('should use ErrorContext with stderr', () => {
    const err = classifyError(new Error('process exited'), {
      error: new Error('process exited'),
      stderr: 'SQLITE_ERROR: database is locked',
    });
    assert.equal(err.category, 'DB_ERROR');
    assert.ok(err.details);
  });

  it('should provide recovery actions for retryable errors', () => {
    const err = classifyError('ECONNREFUSED');
    assert.ok(err.recoveryActions);
    assert.ok(err.recoveryActions!.some(a => a.action === 'retry'));
  });

  it('should provide recovery actions for terminal crash', () => {
    const err = classifyError('Terminal session exited');
    assert.ok(err.recoveryActions);
    assert.ok(err.recoveryActions!.some(a => a.action === 'new_session'));
  });
});
