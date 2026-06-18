import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { type FollowMode } from './follow-mode';

describe('FollowMode', () => {
  describe('state transitions (pure logic)', () => {
    function cycleMode(current: FollowMode): FollowMode {
      if (current === 'off') return 'terminal-follow';
      if (current === 'terminal-follow') return 'file-follow';
      return 'off';
    }

    it('should cycle from off → terminal-follow → file-follow → off', () => {
      let mode: FollowMode = 'off';
      assert.equal(mode, 'off');

      mode = cycleMode(mode);
      assert.equal(mode, 'terminal-follow');

      mode = cycleMode(mode);
      assert.equal(mode, 'file-follow');

      mode = cycleMode(mode);
      assert.equal(mode, 'off');
    });

    it('should know which direction follows', () => {
      assert.equal(isTerminalFollowing('file-follow'), true);
      assert.equal(isTerminalFollowing('off'), false);

      assert.equal(isFileBrowserFollowing('terminal-follow'), true);
      assert.equal(isFileBrowserFollowing('off'), false);
    });
  });
});

function isTerminalFollowing(mode: FollowMode): boolean {
  return mode === 'file-follow';
}

function isFileBrowserFollowing(mode: FollowMode): boolean {
  return mode === 'terminal-follow';
}
