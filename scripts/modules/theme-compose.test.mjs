// DES A41 / T01：单源主题组合与角色集自检。
// 覆盖：组合确定性、重复基础角色反例（行首与行中）、两主题角色集合一致、
// 实际 dist 产物等于预期组合。不修改任何工作区文件。
import { test } from 'node:test';
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { composeThemeCss, findDuplicatedBaseRoles, BASE_THEME_ROLES } from './theme-compose.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const baseCss = readFileSync(join(ROOT, 'extension', 'theme.css'), 'utf8');
const moduleCss = readFileSync(join(ROOT, 'modules/fund/ui/src/styles/theme.css'), 'utf8');

test('composeThemeCss is deterministic: base first, module second, marked as generated', () => {
  const a = composeThemeCss(baseCss, moduleCss);
  const b = composeThemeCss(baseCss, moduleCss);
  assert.equal(a, b);
  assert.ok(a.startsWith('/* composed by scripts/modules/build-ui.mjs'));
  assert.ok(a.indexOf('--natives-bg') < a.indexOf('--color-up'), 'base roles must precede module roles');
  assert.ok(a.includes('/* ===== module theme roles ===== */'));
});

test('module theme must not redeclare base roles (line-start and mid-line forms)', () => {
  assert.deepEqual(findDuplicatedBaseRoles(moduleCss), [], 'fund theme.css must not redeclare base roles');
  assert.deepEqual(findDuplicatedBaseRoles(':root {\n  --natives-bg: #000;\n}'), ['--natives-bg']);
  assert.deepEqual(findDuplicatedBaseRoles('x { color:red; --natives-accent: blue; }'), ['--natives-accent']);
  assert.deepEqual(findDuplicatedBaseRoles('a { color: var(--natives-bg); }'), [], 'var() consumption is not a declaration');
  assert.deepEqual(findDuplicatedBaseRoles('a { --natives-bg-sub: 1; }'), [], 'role-prefixed new names are module-owned');
});

test('archive and volt define the same base role set', () => {
  const rolesIn = (block) => new Set([...block.matchAll(/(?<![\w-])(--natives-[\w-]+)\s*:/g)].map((m) => m[1]));
  const defaultBlock = baseCss.slice(baseCss.indexOf(':root {'), baseCss.indexOf(':root[data-theme="volt"]'));
  const voltBlock = baseCss.slice(baseCss.indexOf(':root[data-theme="volt"]'));
  const defaultRoles = rolesIn(defaultBlock);
  const voltRoles = rolesIn(voltBlock);
  assert.deepEqual([...voltRoles].filter((r) => !defaultRoles.has(r)), [], 'volt must not introduce roles missing from archive default');
  assert.ok(defaultRoles.size >= BASE_THEME_ROLES.length - 2, 'default block carries the base role set');
});

test('composed dist/styles/theme.css equals expected composition of current sources', () => {
  const dist = readFileSync(join(ROOT, 'modules/fund/ui/dist/styles/theme.css'), 'utf8');
  assert.equal(dist, composeThemeCss(baseCss, moduleCss), 'run modules:ui:build after changing theme sources');
});
