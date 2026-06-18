"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.SKILL_SOURCE_DIRS = void 0;
exports.checkSkillHealth = checkSkillHealth;
exports.getDeactivatedPath = getDeactivatedPath;
exports.getSkillNameFromPath = getSkillNameFromPath;
const path = __importStar(require("path"));
// ── Source Directories ──
exports.SKILL_SOURCE_DIRS = [
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
function checkSkillHealth(content) {
    const issues = [];
    // 检查 frontmatter
    if (!content.startsWith('---\n')) {
        issues.push('missing-frontmatter');
    }
    else {
        // 提取 description
        const descMatch = content.match(/^---\n([\s\S]*?)\n---/);
        if (descMatch) {
            const frontmatter = descMatch[1];
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
function getDeactivatedPath(skillPath) {
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
function getSkillNameFromPath(skillPath) {
    return path.basename(path.dirname(skillPath));
}
//# sourceMappingURL=skills-manager.js.map