import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import {
  generateSessionToken,
  validateSessionToken,
  invalidateToken,
  invalidateModuleTokens,
  invalidateAllTokens,
} from './token-manager';
import { IframeSandboxManager } from './iframe-sandbox-manager';
import { buildBridgeSdkScript } from './iframe-sandbox';

describe('IframeSandbox', () => {
  describe('SessionToken', () => {
    it('should generate and validate a token', () => {
      const token = generateSessionToken('test-module');
      assert.ok(token);
      assert.ok(token.includes(':'));

      const valid = validateSessionToken(token, 'test-module');
      assert.equal(valid, true);
    });

    it('should reject token for wrong moduleId', () => {
      const token = generateSessionToken('module-a');
      const valid = validateSessionToken(token, 'module-b');
      assert.equal(valid, false);
    });

    it('should reject invalidated token', () => {
      const token = generateSessionToken('test-module');
      invalidateToken(token);

      const valid = validateSessionToken(token, 'test-module');
      assert.equal(valid, false);
    });

    it('should invalidate all tokens for a module', () => {
      const t1 = generateSessionToken('multi-module');
      const t2 = generateSessionToken('multi-module');

      invalidateModuleTokens('multi-module');

      assert.equal(validateSessionToken(t1, 'multi-module'), false);
      assert.equal(validateSessionToken(t2, 'multi-module'), false);
    });
  });

  describe('IframeSandboxManager', () => {
    before(() => {
      // Clean global token state between test suites
      invalidateAllTokens();
    });

    it('should register and provide a token', () => {
      const mgr = new IframeSandboxManager();
      const result = mgr.register('test-module', null);
      assert.ok(result.token);

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
      assert.ok(token);

      mgr.unregister('test-module');
      assert.equal(mgr.getToken('test-module'), undefined);
    });

    it('should handle re-registration (old token invalidated)', () => {
      // Directly use global functions to verify behavior
      const first = generateSessionToken('test-module');
      const firstHash = first.split(':')[0]!;

      invalidateModuleTokens('test-module');

      const second = generateSessionToken('test-module');
      const secondHash = second.split(':')[0]!;

      // Should have different hashes
      assert.notEqual(firstHash, secondHash);

      // Old token should be invalidated
      assert.equal(validateSessionToken(first, 'test-module'), false);
      // New token should be valid
      assert.equal(validateSessionToken(second, 'test-module'), true);
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
