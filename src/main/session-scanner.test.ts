import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { parseSessionTitle, parseClaudeSessionFile } from './session-scanner';

/**
 * 模拟真实 JSONL 会话日志结构（2026-06 实测确定）：
 * - 顶层 type: user / assistant / system（没有顶层的 tool_use）
 * - tool_use 嵌套在 assistant.message.content[] 数组中
 * - 时间戳在顶层 event.timestamp（ISO 8601 格式）
 */
function makeUserLine(text: string, ts?: string): string {
  return JSON.stringify({
    type: 'user',
    message: { role: 'user', content: text },
    timestamp: ts ?? '2026-06-11T07:00:00.000Z',
  });
}

function makeAssistantLine(
  blocks: Record<string, unknown>[],
  ts?: string,
): string {
  return JSON.stringify({
    type: 'assistant',
    message: { role: 'assistant', content: blocks },
    timestamp: ts ?? '2026-06-11T07:00:05.000Z',
  });
}

function makeToolUse(name: string, input: Record<string, unknown>): Record<string, unknown> {
  return { type: 'tool_use', name, id: `call_${Math.random().toString(36).slice(2)}`, input };
}

describe('SessionScanner', () => {
  describe('parseSessionTitle', () => {
    it('should extract first user message as title (real JSONL format)', () => {
      const lines = [
        JSON.stringify({ type: 'system', message: { content: [{ text: 'System init' }] } }),
        makeUserLine('Fix the login bug please'),
        makeAssistantLine([makeToolUse('Edit', { file_path: '/project/src/file.ts' })]),
      ];
      assert.equal(parseSessionTitle(lines), 'Fix the login bug please');
    });

    it('should support legacy human type', () => {
      const lines = [
        JSON.stringify({ type: 'human', message: { content: [{ text: 'Legacy message' }] } }),
      ];
      assert.equal(parseSessionTitle(lines), 'Legacy message');
    });

    it('should handle content as string (alternative format)', () => {
      const lines = [
        JSON.stringify({ type: 'user', message: { role: 'user', content: 'Direct string content' } }),
      ];
      assert.equal(parseSessionTitle(lines), 'Direct string content');
    });

    it('should truncate titles to 100 chars', () => {
      const longTitle = 'A'.repeat(150);
      const lines = [
        makeUserLine(longTitle),
      ];
      const title = parseSessionTitle(lines);
      assert.equal(title!.length, 100);
    });

    it('should return "Untitled" when no user/human message', () => {
      const lines = [
        JSON.stringify({ type: 'assistant', message: { content: [{ type: 'text', text: 'Hello' }] } }),
      ];
      assert.equal(parseSessionTitle(lines), 'Untitled');
    });

    it('should skip malformed JSON lines', () => {
      const lines = [
        'not json',
        makeUserLine('Hello'),
      ];
      assert.equal(parseSessionTitle(lines), 'Hello');
    });
  });

  describe('parseClaudeSessionFile', () => {
    it('should parse tool_use from assistant.message.content[] (real JSONL structure)', async () => {
      const lines = [
        makeUserLine('Refactor utils', '2026-06-11T07:00:00.000Z'),
        makeAssistantLine(
          [makeToolUse('Edit', { file_path: 'src/utils.ts' })],
          '2026-06-11T07:00:05.000Z',
        ),
        makeAssistantLine(
          [makeToolUse('Write', { file_path: 'src/helper.ts' })],
          '2026-06-11T07:00:10.000Z',
        ),
        makeAssistantLine(
          [makeToolUse('Skill', { skill: 'typescript' })],
          '2026-06-11T07:00:15.000Z',
        ),
      ];

      const session = parseClaudeSessionFile('session-123', '/project', lines);
      assert.equal(session.id, 'session-123');
      assert.equal(session.title, 'Refactor utils');
      assert.ok(session.filesModified.includes('src/utils.ts'));
      assert.ok(session.filesModified.includes('src/helper.ts'));
      assert.ok(session.skillsUsed.includes('typescript'));
      assert.equal(session.filesModified.length, 2);
      assert.equal(session.skillsUsed.length, 1);
    });

    it('should use real timestamps from event.timestamp (not eventIndex*100)', () => {
      const lines = [
        makeUserLine('hi', '2026-06-11T07:00:00.000Z'),
        makeAssistantLine(
          [makeToolUse('Edit', { file_path: 'file.ts' })],
          '2026-06-11T07:05:30.123Z',
        ),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      // 确认 startTime 是真实日期（2026-06-11T07:00:00Z = 1749193200000）
      assert.equal(session.startTime, Date.parse('2026-06-11T07:00:00.000Z'));
      // 确认 fileTimestamps 使用真实时间戳而非等差数列
      assert.equal(session.fileTimestamps['file.ts'], Date.parse('2026-06-11T07:05:30.123Z'));
    });

    it('should deduplicate files and skills', () => {
      const lines = [
        makeUserLine('hi', '2026-06-11T07:00:00.000Z'),
        makeAssistantLine(
          [makeToolUse('Edit', { file_path: 'file.ts' })],
          '2026-06-11T07:00:05.000Z',
        ),
        makeAssistantLine(
          [makeToolUse('Edit', { file_path: 'file.ts' })],
          '2026-06-11T07:00:10.000Z',
        ),
        makeAssistantLine(
          [makeToolUse('Skill', { skill: 'ts' })],
          '2026-06-11T07:00:15.000Z',
        ),
        makeAssistantLine(
          [makeToolUse('Skill', { skill: 'ts' })],
          '2026-06-11T07:00:20.000Z',
        ),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      assert.equal(session.filesModified.length, 1);
      assert.equal(session.fileTimestamps['file.ts'], Date.parse('2026-06-11T07:00:05.000Z'));
      assert.equal(session.skillsUsed.length, 1);
    });

    it('should handle mixed content blocks (text + tool_use)', () => {
      const lines = [
        makeUserLine('hi'),
        makeAssistantLine([
          { type: 'text', text: 'Let me check that...' },
          makeToolUse('Read', { file_path: 'readme.md' }),
          makeToolUse('Edit', { file_path: 'src/main.ts' }),
        ]),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      // Read 不是 Edit/Write/NotebookEdit，不应出现在 filesModified
      assert.ok(!session.filesModified.includes('readme.md'));
      assert.ok(session.filesModified.includes('src/main.ts'));
      assert.equal(session.filesModified.length, 1);
    });

    it('should return empty arrays when no tool_use found', () => {
      const lines = [
        makeUserLine('Hello'),
        makeAssistantLine([{ type: 'text', text: 'Hi there' }]),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      assert.equal(session.filesModified.length, 0);
      assert.equal(session.skillsUsed.length, 0);
      assert.deepEqual(session.fileTimestamps, {});
    });

    it('should handle Thinking blocks before tool_use', () => {
      const lines = [
        makeUserLine('analyze code'),
        makeAssistantLine([
          { type: 'thinking', thinking: 'Let me analyze...' },
          makeToolUse('Edit', { file_path: 'src/analyzer.ts' }),
        ]),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      assert.ok(session.filesModified.includes('src/analyzer.ts'));
    });

    it('should ignore non-matching tool names', () => {
      const lines = [
        makeUserLine('hello'),
        makeAssistantLine([
          makeToolUse('Bash', { command: 'ls' }),
          makeToolUse('Read', { file_path: 'file.ts' }),
          makeToolUse('Agent', { subagent_type: 'code-review' }),
        ]),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      assert.equal(session.filesModified.length, 0);
      assert.equal(session.skillsUsed.length, 0);
    });

    it('should handle Skill tool_use with full input', () => {
      const lines = [
        makeUserLine('run skill'),
        makeAssistantLine([
          makeToolUse('Skill', { skill: 'typescript', config: { strict: true } }),
        ]),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      assert.ok(session.skillsUsed.includes('typescript'));
      assert.equal(session.skillsUsed.length, 1);
    });

    it('should use fallback Date.now() when no timestamp lines exist', () => {
      // 没有时间戳的行
      const lines = [
        JSON.stringify({ type: 'user', message: { role: 'user', content: 'hello' } }),
        JSON.stringify({ type: 'assistant', message: { content: [makeToolUse('Edit', { file_path: 'x.ts' })] } }),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      assert.equal(session.startTime > 1_700_000_000_000, true); // 合理时间
      assert.equal(session.filesModified.length, 1);
    });
  });
});
