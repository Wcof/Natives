// ─── Extension Integration Tests ─────────────────────────
//
// Tests for MCP, Skills, Commands, and Hooks extension families.

import { describe, it, assert } from 'tsx:test';
import { MCPExtension, SkillsExtension, CommandsExtension, HooksExtension } from '../index';

describe('MCPExtension', () => {
  it('should register and list MCP servers', () => {
    const mcp = new MCPExtension();
    mcp.register({ id: 'mcp-1', name: 'Test MCP', transport: 'stdio', command: 'node', args: ['server.js'] });
    assert.equal(mcp.list().length, 1);
  });

  it('should get MCP server by ID', () => {
    const mcp = new MCPExtension();
    mcp.register({ id: 'mcp-2', name: 'Test MCP 2', transport: 'http_sse', url: 'http://localhost:8080' });
    const server = mcp.get('mcp-2');
    assert.ok(server !== undefined);
    assert.equal(server!.name, 'Test MCP 2');
  });

  it('should unregister MCP servers', () => {
    const mcp = new MCPExtension();
    mcp.register({ id: 'mcp-3', name: 'Test MCP 3', transport: 'stdio' });
    mcp.unregister('mcp-3');
    assert.equal(mcp.list().length, 0);
  });
});

describe('SkillsExtension', () => {
  it('should register and execute skills', () => {
    const skills = new SkillsExtension();
    skills.register({ id: 'skill-1', name: 'Test Skill', version: '1.0.0', description: 'A test skill', prompt: 'Hello {{name}}' });
    const result = skills.execute('skill-1', { name: 'World' });
    assert.ok(result.includes('Hello'));
  });

  it('should throw for unknown skills', () => {
    const skills = new SkillsExtension();
    try {
      skills.execute('unknown-skill');
      assert.fail('Should have thrown');
    } catch (e) {
      assert.ok(e instanceof Error);
    }
  });
});

describe('CommandsExtension', () => {
  it('should register and execute commands', () => {
    const cmds = new CommandsExtension();
    cmds.register({ name: 'greet', description: 'Greets the user', handler: (p) => `Hello, ${p.name || 'World'}!` });
    const result = cmds.execute('greet', { name: 'Test' });
    assert.equal(result, 'Hello, Test!');
  });
});

describe('HooksExtension', () => {
  it('should register and run hooks', () => {
    const hooks = new HooksExtension();
    hooks.register({
      id: 'hook-1', hookPoint: 'before_tool_call', priority: 10,
      handler: () => ({ action: 'allow' as const }),
      timeoutMs: 5000,
    });
    const results = hooks.run('before_tool_call', { tool: 'read_file' });
    assert.equal(results.length, 1);
    assert.equal(results[0].action, 'allow');
  });

  it('should stop on block action', () => {
    const hooks = new HooksExtension();
    hooks.register({
      id: 'hook-block', hookPoint: 'before_tool_call', priority: 10,
      handler: () => ({ action: 'block' as const, reason: 'Blocked by policy' }),
      timeoutMs: 5000,
    });
    hooks.register({
      id: 'hook-after', hookPoint: 'before_tool_call', priority: 5,
      handler: () => ({ action: 'allow' as const }),
      timeoutMs: 5000,
    });
    const results = hooks.run('before_tool_call', {});
    assert.equal(results.length, 1, 'Should stop after block');
    assert.equal(results[0].id, 'hook-block');
  });
});