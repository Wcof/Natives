import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { checkSkillHealth, getDeactivatedPath, SKILL_SOURCE_DIRS } from './skills-manager';

describe('SkillsManager', () => {
  it('should have 5 skill source directories', () => {
    assert.equal(SKILL_SOURCE_DIRS.length, 5);
  });

  describe('checkSkillHealth', () => {
    it('should detect missing frontmatter in SKILL.md', () => {
      const content = '# My Skill\nSome description';
      const health = checkSkillHealth(content);
      assert.equal(health.ok, false);
      assert.ok(health.issues.includes('missing-frontmatter'));
    });

    it('should detect truncated description (>1536 chars)', () => {
      const description = 'A'.repeat(1600);
      const content = `---\nname: test\ndescription: ${description}\n---\n# Test`;
      const health = checkSkillHealth(content);
      assert.equal(health.ok, false);
      assert.ok(health.issues.includes('description-truncated'));
    });

    it('should return ok for healthy skill', () => {
      const content = `---\nname: test\ndescription: A test skill\n---\n# Test\n\nSome content here.`;
      const health = checkSkillHealth(content);
      assert.equal(health.ok, true);
      assert.equal(health.issues.length, 0);
    });
  });

  describe('getDeactivatedPath', () => {
    it('should return _disabled path for a skill', () => {
      const result = getDeactivatedPath('/home/user/.claude/skills/my-skill/SKILL.md');
      assert.ok(result.endsWith('_disabled/skills/my-skill/SKILL.md'));
    });
  });
});

export {};
