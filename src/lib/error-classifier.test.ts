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
  });

  it('should classify timeout', () => {
    const err = classifyError('Request timed out after 30s');
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
});
