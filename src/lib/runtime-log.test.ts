import { describe, it, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { initRuntimeLog, getRecentLogs, clearLogs, getRecentLogsText } from './runtime-log';

describe('RuntimeLog', () => {
  beforeEach(() => {
    clearLogs();
  });

  it('should install without errors', () => {
    initRuntimeLog();
    // Safe to call multiple times
    initRuntimeLog();
  });

  it('should capture console.error', () => {
    initRuntimeLog();
    const before = getRecentLogs().length;
    console.error('test error message');
    const logs = getRecentLogs();
    assert.ok(logs.length > before);
    const last = logs[logs.length - 1]!;
    assert.equal(last.level, 'error');
    assert.ok(last.message.includes('test error message'));
    assert.ok(last.timestamp);
  });

  it('should capture console.warn', () => {
    initRuntimeLog();
    console.warn('test warning');
    const logs = getRecentLogs();
    const last = logs[logs.length - 1]!;
    assert.equal(last.level, 'warn');
  });

  it('should scrub API keys', () => {
    initRuntimeLog();
    console.error('Failed with key sk-abc12345xyz789secret');
    const logs = getRecentLogs();
    const last = logs[logs.length - 1]!;
    assert.ok(last.message.includes('sk-abc12'));
    assert.ok(!last.message.includes('xyz789secret'));
  });

  it('should scrub Bearer tokens', () => {
    initRuntimeLog();
    console.error('Auth failed: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U');
    const logs = getRecentLogs();
    const last = logs[logs.length - 1]!;
    assert.ok(!last.message.includes('eyJhbGci'));
    assert.ok(last.message.includes('Bearer'));
  });

  it('should cap entry length at 2000 chars', () => {
    initRuntimeLog();
    console.error('x'.repeat(5000));
    const logs = getRecentLogs();
    const last = logs[logs.length - 1]!;
    assert.ok(last.message.length <= 2000);
  });

  it('should clear logs', () => {
    initRuntimeLog();
    console.error('test');
    assert.ok(getRecentLogs().length > 0);
    clearLogs();
    assert.equal(getRecentLogs().length, 0);
  });

  it('should format logs as text', () => {
    initRuntimeLog();
    console.error('formatted test');
    const text = getRecentLogsText();
    assert.ok(text.includes('ERROR'));
    assert.ok(text.includes('formatted test'));
  });
});
