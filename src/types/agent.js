"use strict";
// ── Agent Types ──
Object.defineProperty(exports, "__esModule", { value: true });
exports.SKILL_ISSUES = exports.SKILL_SOURCES = void 0;
// ── Constants ──
/** 所有 Skill 来源 */
exports.SKILL_SOURCES = [
    '~/.claude/skills',
    'project/.claude/skills',
    'claude-plugins',
    '~/.codex/skills',
    '~/.agents/skills',
];
/** 所有可能的 Skill 问题 */
exports.SKILL_ISSUES = [
    'description-truncated',
    'missing-frontmatter',
    'missing-skill-md',
    'residue-files',
];
//# sourceMappingURL=agent.js.map