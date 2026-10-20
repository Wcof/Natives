// 单源主题组合规则（DES 02 / DES-D07 / A41）：
// build-ui 构建期组合、check-ui 严格校验与 A41 维护演练共用同一实现，
// 避免「检查器与构建器各写一份规则」造成漂移。
import { readFileSync } from 'node:fs';

export function composeThemeCss(baseCss, moduleCss) {
  return `/* composed by scripts/modules/build-ui.mjs — do not edit (source: extension/theme.css + modules/<appId>/ui/src/styles/theme.css) */\n${baseCss}\n/* ===== module theme roles ===== */\n${moduleCss}`;
}

// 基础角色（extension/theme.css 拥有）：模块主题重复声明即失败。
export const BASE_THEME_ROLES = [
  '--natives-bg', '--natives-surface', '--natives-surface-2', '--natives-surface-3',
  '--natives-text-primary', '--natives-text-muted', '--natives-border', '--natives-control',
  '--natives-accent', '--natives-accent-strong', '--natives-on-accent', '--natives-selected',
  '--natives-danger', '--natives-danger-soft', '--natives-on-danger',
  '--natives-warning', '--natives-warning-soft', '--natives-ok', '--natives-ok-soft',
  '--natives-code', '--natives-match', '--natives-backdrop', '--natives-shadow',
  '--natives-glass', '--natives-glass-hover', '--natives-disabled', '--natives-focus-width',
  '--natives-font', '--natives-font-mono',
];

export function findDuplicatedBaseRoles(moduleCss) {
  return BASE_THEME_ROLES.filter((role) =>
    new RegExp(`(?<![\\w-])${role.replace(/-/g, '\\-')}\\s*:`, 'm').test(moduleCss));
}

export function readBaseThemeCss(root) {
  return readFileSync(`${root}/extension/theme.css`, 'utf8');
}
