import * as path from 'path';
import * as fs from 'fs';
import { type SkillSource, type SkillHealth, type SkillIssue } from '../types/agent';

// ── Source Directories（单一来源：types/agent.ts 的 SKILL_SOURCES） ──

// 重新导出，保持向后兼容
export { SKILL_SOURCES as SKILL_SOURCE_DIRS } from '../types/agent';

// ── Health Check ──

/**
 * 检查 SKILL.md 的健康状态
 *
 * 改进点（对标 fanbox）：
 * - 支持 BOM 开头的文件
 * - 支持 CRLF 行尾
 * - 检测 missing-skill-md（空内容）
 * - YAML frontmatter 解析更健壮
 */
export function checkSkillHealth(content: string): SkillHealth {
  const issues: SkillIssue[] = [];

  // 去除 BOM（U+FEFF = UTF-8 Byte Order Mark）
  const clean = content.replace(/^\uFEFF/, '');

  // 空内容 = missing-skill-md
  if (!clean.trim()) {
    issues.push('missing-skill-md');
    return { ok: false, issues };
  }

  // 检查 frontmatter（支持 \n 和 \r\n）
  const fmMatch = clean.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!fmMatch) {
    issues.push('missing-frontmatter');
    return { ok: false, issues };
  }

  // 提取 description（支持单行值，忽略多行块标量）
  const frontmatter = fmMatch[1]!;
  const lines = frontmatter.split(/\r?\n/);
  for (const line of lines) {
    const descMatch = line.match(/^description:\s*(.+)/);
    if (descMatch) {
      const desc = descMatch[1]!.trim();
      // 去除引号
      const unquoted = desc.replace(/^["']|["']$/g, '');
      if (unquoted.length > 1536) {
        issues.push('description-truncated');
      }
      break;
    }
  }

  return { ok: issues.length === 0, issues };
}

// ── Deactivation / Activation ──

/**
 * 获取禁用路径（移动到 _disabled/ 目录）
 *
 * 输入: ~/.claude/skills/my-skill/SKILL.md
 * 输出: ~/.claude/_disabled/skills/my-skill/SKILL.md
 */
export function getDeactivatedPath(skillPath: string): string {
  const parts = skillPath.split(path.sep);
  const skillsIdx = parts.findIndex((p) => p === 'skills');
  if (skillsIdx !== -1 && skillsIdx + 1 < parts.length) {
    parts.splice(skillsIdx, 0, '_disabled');
  }
  return parts.join(path.sep);
}

/**
 * 获取激活路径（从 _disabled/ 目录恢复）
 *
 * 输入: ~/.claude/_disabled/skills/my-skill/SKILL.md
 * 输出: ~/.claude/skills/my-skill/SKILL.md
 */
export function getActivatedPath(skillPath: string): string {
  const parts = skillPath.split(path.sep);
  const disabledIdx = parts.findIndex((p) => p === '_disabled');
  if (disabledIdx !== -1) {
    parts.splice(disabledIdx, 1);
  }
  return parts.join(path.sep);
}

/**
 * 从 SKILL.md 路径解析技能名称
 */
export function getSkillNameFromPath(skillPath: string): string {
  return path.basename(path.dirname(skillPath));
}

// ── Symlink-safe skill toggle（对标 fanbox 的 symlink 处理） ──

/**
 * 切换技能启用/禁用状态（安全处理符号链接）
 *
 * 对标 fanbox 的 skillToggle 模式：
 * - 使用 lstat 检测符号链接
 * - 使用 realpath 解析真实路径
 * - 符号链接：先 unlink 再 symlink 到新位置
 * - 普通目录：直接 rename
 *
 * @param skillPath - SKILL.md 的完整路径（如 ~/.claude/skills/my-skill/SKILL.md）
 * @param enable - true=启用（从 _disabled/ 移回），false=禁用（移到 _disabled/）
 */
export async function toggleSkill(skillPath: string, enable: boolean): Promise<void> {
  const skillDir = path.dirname(skillPath);
  const sourcePath = enable ? getActivatedPath(skillPath) : skillDir;
  const destPath = enable ? skillDir : getDeactivatedPath(skillPath);

  // 确保目标父目录存在
  const destParent = path.dirname(destPath);
  await fs.promises.mkdir(destParent, { recursive: true });

  // 检测符号链接
  const lst = await fs.promises.lstat(sourcePath);
  if (lst.isSymbolicLink()) {
    // 符号链接：解析真实目标，删除旧链接，在新位置创建链接
    const realTarget = await fs.promises.realpath(sourcePath);
    await fs.promises.unlink(sourcePath);
    await fs.promises.symlink(realTarget, destPath);
  } else {
    // 普通目录/文件：直接移动
    await fs.promises.rename(sourcePath, destPath);
  }
}
