import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { detectAgentStatus } from './agent-status';

describe('detectAgentStatus', () => {
  it('should return "running" for normal terminal output without exit code', () => {
    assert.equal(detectAgentStatus('Hello world\nBuilding project...', undefined), 'running');
    assert.equal(detectAgentStatus('Compiling TypeScript...\nDone!', undefined), 'running');
  });

  it('should detect "idle" when seeing command prompt patterns', () => {
    assert.equal(detectAgentStatus('user@host:~$ ', undefined), 'idle');
    assert.equal(detectAgentStatus('/project/src $ ', undefined), 'idle');
    assert.equal(detectAgentStatus('λ node server.js', undefined), 'idle');
  });

  it('should detect "exited" from non-zero exit codes', () => {
    assert.equal(detectAgentStatus('', 0), 'idle');
    assert.equal(detectAgentStatus('', 1), 'exited');
    assert.equal(detectAgentStatus('', 2), 'exited');
  });

  it('should detect "exited" from error keywords', () => {
    assert.equal(detectAgentStatus('Error: command not found', undefined), 'exited');
    assert.equal(detectAgentStatus('Killed: 9', undefined), 'exited');
  });
});
