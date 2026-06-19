import { describe, it, before } from 'node:test';
import assert from 'node:assert/strict';
import { IframeSandboxManager } from './iframe-sandbox-manager';
import { buildBridgeSdkScript } from './iframe-sandbox';

describe('IframeSandbox', () => {
  // Token generation/validation moved to src-tauri/token_manager.rs (HMAC-SHA256)
  // Renderer-side tests only cover IframeSandboxManager and BridgeSdkScript.

  describe('IframeSandboxManager', () => {
    before(() => {
      // Clean any state between test suites
    });

    it('should register and provide a token', () => {
      const mgr = new IframeSandboxManager();
      const result = mgr.register('test-module', null);
      // Token generation moved to main process; renderer returns empty string
      assert.equal(typeof result.token, 'string');

      const storedToken = mgr.getToken('test-module');
      assert.equal(storedToken, result.token);
    });

    it('should verify message source', () => {
      const mgr = new IframeSandboxManager();
      const mockWindow = {} as Window;

      mgr.register('module-a', mockWindow);
      assert.equal(mgr.verifyMessageSource('module-a', mockWindow), true);
      assert.equal(mgr.verifyMessageSource('module-a', null), false);
    });

    it('should update heartbeat', () => {
      const mgr = new IframeSandboxManager();
      mgr.register('test-module', null);

      const before = mgr.getTimeoutCount('test-module', 10000);
      assert.equal(before, 0);

      mgr.updateHeartbeat('test-module');
    });

    it('should unregister and invalidate tokens', () => {
      const mgr = new IframeSandboxManager();
      mgr.register('test-module', null);
      const token = mgr.getToken('test-module');
      // Token may be empty string (generation moved to main process)
      assert.equal(typeof token, 'string');

      mgr.unregister('test-module');
      assert.equal(mgr.getToken('test-module'), undefined);
    });

    it('should handle re-registration', () => {
      const mgr = new IframeSandboxManager();
      mgr.register('test-module', null);
      const first = mgr.getToken('test-module');

      mgr.register('test-module', null);
      const second = mgr.getToken('test-module');

      // Both should be empty strings (token generation moved to main process)
      assert.equal(first, '');
      assert.equal(second, '');
    });
  });

  describe('BridgeSdkScript', () => {
    it('should generate valid JS SDK script', () => {
      const script = buildBridgeSdkScript(3456, 'http://localhost:3456');
      assert.ok(script.includes('window.natives'));
      assert.ok(script.includes('3456'));
      assert.ok(script.includes('bridgeRequest'));
      assert.ok(script.includes('token-request'));
      assert.ok(script.includes('http://localhost:3456'));
      assert.ok(!script.includes("'*'"));
    });
  });
});
