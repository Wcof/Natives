import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  type AgentProject,
  type AgentSession,
  type AgentStatus,
  type SkillInfo,
  type FileChangeEvent,
  SKILL_SOURCES,
} from './agent';

describe('AgentTypes', () => {
  it('should construct a valid AgentProject', () => {
    const project: AgentProject = {
      path: '/home/user/project',
      name: 'my-project',
      engine: 'claude',
      lastActive: Date.now(),
      sessionCount: 5,
    };
    assert.equal(project.name, 'my-project');
    assert.equal(project.engine, 'claude');
  });

  it('should construct a valid AgentSession (real jsonl transcript shape)', () => {
    const session: AgentSession = {
      id: 'df628f5a-b96b-4e21-8c30-e9d2bdedabb4',
      path: '/home/user/.claude/projects/-home-user-project/df628f5a.jsonl',
      mtimeMs: Date.now(),
      size: 4096,
      title: 'Fix login bug',
    };
    assert.equal(session.title, 'Fix login bug');
    assert.ok(session.size > 0);
  });

  it('should allow AgentSession without a title', () => {
    const session: AgentSession = {
      id: 'abc',
      path: '/x/abc.jsonl',
      mtimeMs: 0,
      size: 0,
      title: null,
    };
    assert.equal(session.title, null);
  });

  it('should construct a valid SkillInfo', () => {
    const skill: SkillInfo = {
      name: 'typescript',
      description: 'TypeScript development helper',
      source: '~/.claude/skills',
      path: '/home/user/.claude/skills/typescript/SKILL.md',
      enabled: true,
      health: { ok: true, issues: [] },
      triggerCount: 42,
      lastTriggered: Date.now() - 86400000,
    };
    assert.equal(skill.triggerCount, 42);
    assert.equal(skill.health.ok, true);
  });

  it('should report health issues', () => {
    const skill: SkillInfo = {
      name: 'broken-skill',
      description: '',
      source: 'project/.claude/skills',
      path: '/project/.claude/skills/broken/SKILL.md',
      enabled: false,
      health: { ok: false, issues: ['missing-frontmatter', 'description-truncated'] },
      triggerCount: 0,
    };
    assert.equal(skill.health.ok, false);
    assert.ok(skill.health.issues.includes('missing-frontmatter'));
  });

  it('should construct FileChangeEvent', () => {
    const event: FileChangeEvent = {
      path: '/project/src/file.ts',
      type: 'modify',
      timestamp: Date.now(),
    };
    assert.equal(event.type, 'modify');
  });

  it('should have 5 skill sources', () => {
    assert.equal(SKILL_SOURCES.length, 5);
    assert.ok(SKILL_SOURCES.includes('~/.claude/skills'));
    assert.ok(SKILL_SOURCES.includes('~/.codex/skills'));
  });

  it('should accept valid AgentStatus values', () => {
    const statuses: AgentStatus[] = ['running', 'idle', 'exited'];
    assert.equal(statuses.length, 3);
  });
});
