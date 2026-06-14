import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { parseSessionTitle, parseClaudeSessionFile } from './session-scanner';

describe('SessionScanner', () => {
  describe('parseSessionTitle', () => {
    it('should extract first human message as title', () => {
      const lines = [
        JSON.stringify({ type: 'system', message: { content: [{ text: 'System init' }] } }),
        JSON.stringify({ type: 'human', message: { content: [{ text: 'Fix the login bug please' }] } }),
        JSON.stringify({ type: 'tool_use', name: 'Edit', input: { file_path: '/project/src/file.ts' } }),
      ];
      assert.equal(parseSessionTitle(lines), 'Fix the login bug please');
    });

    it('should truncate titles to 100 chars', () => {
      const longTitle = 'A'.repeat(150);
      const lines = [
        JSON.stringify({ type: 'human', message: { content: [{ text: longTitle }] } }),
      ];
      const title = parseSessionTitle(lines);
      assert.equal(title!.length, 100);
    });

    it('should return "Untitled" when no human message', () => {
      const lines = [
        JSON.stringify({ type: 'tool_use', name: 'Edit', input: { file_path: 'test.ts' } }),
      ];
      assert.equal(parseSessionTitle(lines), 'Untitled');
    });
  });

  describe('parseClaudeSessionFile', () => {
    it('should parse session metadata from JSONL lines', async () => {
      const lines = [
        JSON.stringify({ type: 'human', message: { content: [{ text: 'Refactor utils' }] } }),
        JSON.stringify({ type: 'tool_use', name: 'Edit', input: { file_path: 'src/utils.ts' } }),
        JSON.stringify({ type: 'tool_use', name: 'Write', input: { file_path: 'src/helper.ts' } }),
        JSON.stringify({ type: 'tool_use', name: 'Skill', input: { skill: 'typescript' } }),
      ];

      const session = parseClaudeSessionFile('session-123', '/project', lines);
      assert.equal(session.id, 'session-123');
      assert.equal(session.title, 'Refactor utils');
      assert.ok(session.filesModified.includes('src/utils.ts'));
      assert.ok(session.skillsUsed.includes('typescript'));
      assert.equal(session.filesModified.length, 2);
    });

    it('should deduplicate files and skills', () => {
      const lines = [
        JSON.stringify({ type: 'human', message: { content: [{ text: 'hi' }] } }),
        JSON.stringify({ type: 'tool_use', name: 'Edit', input: { file_path: 'file.ts' } }),
        JSON.stringify({ type: 'tool_use', name: 'Edit', input: { file_path: 'file.ts' } }),
        JSON.stringify({ type: 'tool_use', name: 'Skill', input: { skill: 'ts' } }),
        JSON.stringify({ type: 'tool_use', name: 'Skill', input: { skill: 'ts' } }),
      ];
      const session = parseClaudeSessionFile('s1', '/p', lines);
      assert.equal(session.filesModified.length, 1);
      assert.equal(session.skillsUsed.length, 1);
    });
  });
});
