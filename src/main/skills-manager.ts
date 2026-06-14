import * as path from 'path';
import { type SkillSource, type SkillHealth, type SkillIssue } from '../types/agent';

// ── Source Directories ──

export const SKILL_SOURCE_DIRS: SkillSource[] = [
  '~/.claude/skills',
  'project/.claude/skills',
  'claude-plugins',
  '~/.codex/skills',
  '~/.agents/skills',
];

// ── Health Check ──

/**
 * 检查 SKILL.md 的健康状态
 */
export function checkSkillHealth(content: string): SkillHealth {
  const issues: SkillIssue[] = [];

  // 检查 frontmatter
  if (!content.startsWith('---\n')) {
    issues.push('missing-frontmatter');
  } else {
    // 提取 description
    const descMatch = content.match(/^---\n([\s\S]*?)\n---/);
    if (descMatch) {
      const frontmatter = descMatch[1]!;
      const descLine = frontmatter.split('\n').find((l) => l.startsWith('description:'));
      if (descLine) {
        const desc = descLine.split(':').slice(1).join(':').trim();
        if (desc.length > 1536) {
          issues.push('description-truncated');
        }
      }
    }
  }

  return { ok: issues.length === 0, issues };
}

// ── Deactivation ──

/**
 * 获取禁用路径（移动到 _disabled/ 目录）
 */
export function getDeactivatedPath(skillPath: string): string {
  const parts = skillPath.split(path.sep);
  const skillNameIdx = parts.findIndex((p) => p === 'skills');
  if (skillNameIdx !== -1 && skillNameIdx + 1 < parts.length) {
    parts.splice(skillNameIdx, 0, '_disabled');
  }
  return parts.join(path.sep);
}

/**
 * 从 SKILL.md 路径解析技能名称
 */
export function getSkillNameFromPath(skillPath: string): string {
  return path.basename(path.dirname(skillPath));
}
