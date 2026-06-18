import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  type AgentProject,
  type AgentSession,
  type AgentStatus,
  type SkillInfo,
  type SkillSource,
  type SkillHealth,
  type ClaudeUsage,
  type CodexUsage,
  type RtkUsage,
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

  it('should construct a valid AgentSession', () => {
    const session: AgentSession = {
      id: 'session-123',
      engine: 'codex',
      projectPath: '/home/user/project',
      title: 'Fix login bug',
      startTime: Date.now(),
      endTime: Date.now() + 3600000,
      filesModified: ['src/login.ts', 'src/auth.ts'],
      fileTimestamps: { 'src/login.ts': 0, 'src/auth.ts': 100 },
      skillsUsed: ['typescript', 'debugging'],
    };
    assert.equal(session.title, 'Fix login bug');
    assert.ok(session.filesModified.length >= 2);
  });

  it('should handle Codex engine', () => {
    const session: AgentSession = {
      id: 'codex-session-1',
      engine: 'codex',
      projectPath: '/project',
      title: 'Codex task',
      startTime: Date.now(),
      filesModified: [],
      fileTimestamps: {},
      skillsUsed: [],
    };
    assert.equal(session.engine, 'codex');
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

  it('should construct ClaudeUsage', () => {
    const usage: ClaudeUsage = {
      models: {
        'claude-sonnet': {
          inputTokens: 10000,
          outputTokens: 2000,
          cacheReadInputTokens: 50000,
          cacheCreationInputTokens: 0,
          costUSD: 0.05,
        },
      },
      localTokens: { today: 45000, thisWeek: 200000, total: 1000000 },
      activity: { totalSessions: 64, totalMessages: 23074, firstSessionDate: '2026-03-23' },
    };
    assert.equal(usage.models['claude-sonnet']?.inputTokens, 10000);
    assert.equal(usage.localTokens.total, 1000000);
    assert.equal(usage.activity.totalSessions, 64);
  });

  it('should construct CodexUsage', () => {
    const usage: CodexUsage = {
      fiveHourWindow: { used: 5000, limit: 30000, resetAt: Date.now() + 18000000 },
      planType: 'Pro',
    };
    assert.equal(usage.planType, 'Pro');
  });

  it('should construct FileChangeEvent', () => {
    const event: FileChangeEvent = {
      path: '/project/src/file.ts',
      type: 'modify',
      timestamp: Date.now(),
      sessionId: 'term-1',
    };
    assert.equal(event.type, 'modify');
    assert.equal(event.sessionId, 'term-1');
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
