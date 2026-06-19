import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// Test the shell module's pure logic (PTY requires native module, tested in integration)
describe('Shell', () => {
  it('should have session management functions', () => {
    // Verify the module exports exist (actual PTY tests require electron/node-pty)
    const mod = require('./shell');
    assert.equal(typeof mod.createSession, 'function');
    assert.equal(typeof mod.write, 'function');
    assert.equal(typeof mod.resize, 'function');
    assert.equal(typeof mod.killSession, 'function');
    assert.equal(typeof mod.getActiveSessions, 'function');
    assert.equal(typeof mod.getSessionCount, 'function');
  });

  it('should start with zero sessions', () => {
    const mod = require('./shell');
    assert.equal(mod.getSessionCount(), 0);
  });

  it('should create a session using fallback spawn when pty is unavailable', async () => {
    const mod = require('./shell');
    const sessionId = await mod.createSession({});
    assert.ok(sessionId, 'session id should be truthy');
    assert.equal(mod.getSessionCount(), 1);
    mod.killSession(sessionId);
    assert.equal(mod.getSessionCount(), 0);
  });
});
