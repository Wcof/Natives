/**
 * W1 mutation fixtures for the i18n gate (fail-closed).
 * Proves the OLD implementation would false-green on each case and the NEW
 * implementation FAILs. Run: node --test scripts/i18n-check.test.mjs
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const i18n = require('./i18n-check.mjs');

function makeFixture(t) {
  return mkdtempSync(join(tmpdir(), `i18n-w1-${t}-`));
}
function cleanup(dir) {
  rmSync(dir, { recursive: true, force: true });
}
function writeI18nTree(dir, zh, en) {
  mkdirSync(join(dir, 'src/i18n'), { recursive: true });
  writeFileSync(join(dir, 'src/i18n/zh.ts'), zh);
  writeFileSync(join(dir, 'src/i18n/en.ts'), en);
}
function writeProductionFile(dir, relativePath, content) {
  const path = join(dir, 'src', relativePath);
  mkdirSync(join(path, '..'), { recursive: true });
  writeFileSync(path, content);
}

// I1: empty locale object must fail (old logic: extractKeys returned empty Set
// for non-object input -> zh=0, en=0 -> "in sync" false green)
test('I1: empty zh locale object is a fatal', () => {
  const dir = makeFixture('i1');
  try {
    writeI18nTree(dir, 'export const zh = {};\n', 'export const en = {};\n');
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1, 'empty locale must fail');
    assert.ok(res.emptyFiles.includes('zh'), 'zh flagged as empty');
  } finally {
    cleanup(dir);
  }
});

// I2: unparseable locale file (invalid object literal) must fail, not 0=0
test('I2: invalid object literal is a fatal parse error', () => {
  const dir = makeFixture('i2');
  try {
    writeI18nTree(dir, 'export const zh = { a: ; }\n', 'export const en = { a: 1 };\n');
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1, 'syntax-invalid locale must fail');
  } finally {
    cleanup(dir);
  }
});

// I3: missing key between zh/en must fail
test('I3: zh key missing in en fails', () => {
  const dir = makeFixture('i3');
  try {
    writeI18nTree(dir, 'export const zh = { hello: "你好" };\n', 'export const en = {};\n');
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1);
    assert.ok(res.missingInEn.includes('hello'));
  } finally {
    cleanup(dir);
  }
});

// I4: duplicate key inside one locale is a defect (last-write-wins would drop copy)
test('I4: duplicate key in zh fails', () => {
  const dir = makeFixture('i4');
  try {
    writeI18nTree(
      dir,
      'export const zh = { a: "一", a: "二" };\n',
      'export const en = { a: "one" };\n',
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1, 'duplicate key must fail');
    assert.ok(res.violations.some((v) => v.includes('duplicate')), 'violation mentions duplicate');
  } finally {
    cleanup(dir);
  }
});

// I5: spread composition root resolves domain files; missing domain file fails
test('I5: missing spread domain file is a fatal', () => {
  const dir = makeFixture('i5');
  try {
    writeI18nTree(
      dir,
      'export const zh = { ...app, ...nav };\n',
      'export const en = { ...app, ...nav };\n',
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1, 'missing domain file must fail');
  } finally {
    cleanup(dir);
  }
});

// I6: spread composition resolves real domain files correctly
test('I6: spread composition resolves real domain files', () => {
  const dir = makeFixture('i6');
  try {
    mkdirSync(join(dir, 'src/i18n/zh'), { recursive: true });
    mkdirSync(join(dir, 'src/i18n/en'), { recursive: true });
    writeFileSync(join(dir, 'src/i18n/zh/app.ts'), 'export const app = { home: "首页", nav: { back: "返回" } };\n');
    writeFileSync(join(dir, 'src/i18n/en/app.ts'), 'export const app = { home: "Home", nav: { back: "Back" } };\n');
    writeFileSync(join(dir, 'src/i18n/zh.ts'), 'export const zh = { ...app };\n');
    writeFileSync(join(dir, 'src/i18n/en.ts'), 'export const en = { ...app };\n');
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, 'keys in sync via spread must pass');
    assert.equal(res.zhKeys, 2);
    assert.equal(res.enKeys, 2);
  } finally {
    cleanup(dir);
  }
});

// I7: comment-leading content parses clean (not a false positive)
test('I7: comment headers do not break parsing', () => {
  const dir = makeFixture('i7');
  try {
    writeI18nTree(
      dir,
      '// Copyright\n/* block */\nexport const zh = { save: "保存" };\n',
      'export const en = { save: "Save" };\n',
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, 'comment-leading locale must pass');
  } finally {
    cleanup(dir);
  }
});

test('I8: valid direct and aliased wrapper literal keys pass', () => {
  const dir = makeFixture('i8');
  try {
    writeI18nTree(
      dir,
      'export const zh = { common: { copy: "复制" } };\n',
      'export const en = { common: { copy: "Copy" } };\n',
    );
    writeProductionFile(
      dir,
      'components/Valid.tsx',
      [
        "import { t as tr } from '@/i18n';",
        "const translate = (key: string) => tr('zh', key);",
        "export const direct = tr('zh', 'common.copy');",
        "export const wrapped = translate('common.copy');",
      ].join('\n'),
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, res.violations.join('\n'));
  } finally {
    cleanup(dir);
  }
});

test('I9: missing literal key fails with file, line, and key', () => {
  const dir = makeFixture('i9');
  try {
    writeI18nTree(dir, 'export const zh = { common: { save: "保存" } };\n', 'export const en = { common: { save: "Save" } };\n');
    writeProductionFile(
      dir,
      'components/Broken.tsx',
      "import { t } from '@/i18n';\nexport const label = t('zh', 'common.copy');\n",
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1);
    assert.ok(
      res.violations.includes('missing callsite key: src/components/Broken.tsx:2:common.copy'),
      res.violations.join('\n'),
    );
  } finally {
    cleanup(dir);
  }
});

test('I10: dynamic and substitution-template keys are not reported', () => {
  const dir = makeFixture('i10');
  try {
    writeI18nTree(dir, 'export const zh = { common: { save: "保存" } };\n', 'export const en = { common: { save: "Save" } };\n');
    writeProductionFile(
      dir,
      'components/Dynamic.tsx',
      [
        "import { t } from '@/i18n';",
        "declare const key: string;",
        "declare const flag: boolean;",
        "declare const suffix: string;",
        "export const dynamic = t('zh', key);",
        'export const template = t(\'zh\', `common.${suffix}`);',
      ].join('\n'),
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, res.violations.join('\n'));
  } finally {
    cleanup(dir);
  }
});

test('I15: statically enumerable conditional keys audit every branch', () => {
  const dir = makeFixture('i15');
  try {
    writeI18nTree(dir, 'export const zh = { common: { save: "保存" } };\n', 'export const en = { common: { save: "Save" } };\n');
    writeProductionFile(
      dir,
      'components/Conditional.tsx',
      [
        "import { t } from '@/i18n';",
        "declare const flag: boolean;",
        "export const label = t('zh', flag ? 'common.save' : 'common.missing');",
      ].join('\n'),
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1, 'missing conditional branch must fail');
    assert.ok(
      res.violations.includes('missing callsite key: src/components/Conditional.tsx:3:common.missing'),
      res.violations.join('\n'),
    );
    assert.equal(
      res.violations.filter((violation) => violation.includes('common.save')).length,
      0,
      'valid conditional branch must not be reported',
    );
  } finally {
    cleanup(dir);
  }
});

test('I16: parenthesized and short-circuit literal branches are audited', () => {
  const dir = makeFixture('i16');
  try {
    writeI18nTree(dir, 'export const zh = { common: { save: "保存" } };\n', 'export const en = { common: { save: "Save" } };\n');
    writeProductionFile(
      dir,
      'components/Composite.tsx',
      [
        "import { t } from '@/i18n';",
        "declare const flag: boolean;",
        "declare const key: string;",
        "export const parenthesized = t('zh', (flag ? 'common.save' : 'common.missing'));",
        "export const fallback = t('zh', key || 'common.missing');",
        "export const guarded = t('zh', flag && 'common.missing');",
      ].join('\n'),
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 1, 'missing composite branches must fail');
    assert.equal(
      res.violations.filter((violation) => violation.includes('common.missing')).length,
      3,
      res.violations.join('\n'),
    );
  } finally {
    cleanup(dir);
  }
});

test('I11: test and __tests__ callsites are excluded', () => {
  const dir = makeFixture('i11');
  try {
    writeI18nTree(dir, 'export const zh = { common: { save: "保存" } };\n', 'export const en = { common: { save: "Save" } };\n');
    const invalidCall = "import { t } from '@/i18n';\nexport const label = t('zh', 'common.missing');\n";
    writeProductionFile(dir, 'components/Excluded.test.tsx', invalidCall);
    writeProductionFile(dir, 'components/Excluded.spec.tsx', invalidCall);
    writeProductionFile(dir, 'components/__tests__/Excluded.tsx', invalidCall);
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, res.violations.join('\n'));
  } finally {
    cleanup(dir);
  }
});

test('I12: callsite audit uses keys from composed namespaces', () => {
  const dir = makeFixture('i12');
  try {
    mkdirSync(join(dir, 'src/i18n/zh'), { recursive: true });
    mkdirSync(join(dir, 'src/i18n/en'), { recursive: true });
    writeFileSync(join(dir, 'src/i18n/zh/settings.ts'), 'export const settings = { settings: { executionEngine: { title: "执行引擎" } } };\n');
    writeFileSync(join(dir, 'src/i18n/en/settings.ts'), 'export const settings = { settings: { executionEngine: { title: "Execution engine" } } };\n');
    writeFileSync(join(dir, 'src/i18n/zh.ts'), 'export const zh = { ...settings };\n');
    writeFileSync(join(dir, 'src/i18n/en.ts'), 'export const en = { ...settings };\n');
    writeProductionFile(
      dir,
      'components/Settings.tsx',
      "import { t } from '@/i18n';\nexport const title = t('zh', 'settings.executionEngine.title');\n",
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, res.violations.join('\n'));
  } finally {
    cleanup(dir);
  }
});

test('I13: a shadowed translation function name is not audited', () => {
  const dir = makeFixture('i13');
  try {
    writeI18nTree(dir, 'export const zh = { common: { save: "保存" } };\n', 'export const en = { common: { save: "Save" } };\n');
    writeProductionFile(
      dir,
      'components/Shadowed.tsx',
      [
        "import { t } from '@/i18n';",
        "export const valid = t('zh', 'common.save');",
        "export function parse() {",
        "  const t = (value: string) => Date.parse(value);",
        "  return t('not.an.i18n.key');",
        "}",
      ].join('\n'),
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, res.violations.join('\n'));
  } finally {
    cleanup(dir);
  }
});

test('I14: a local module named i18n is not treated as the translation authority', () => {
  const dir = makeFixture('i14');
  try {
    writeI18nTree(dir, 'export const zh = { common: { save: "保存" } };\n', 'export const en = { common: { save: "Save" } };\n');
    writeProductionFile(
      dir,
      'components/Feature.tsx',
      [
        "import { t } from '@/feature/i18n';",
        "export const parsed = t('not.a.locale.key');",
      ].join('\n'),
    );
    const res = i18n.runI18nCheck(dir);
    assert.equal(res.exitCode, 0, res.violations.join('\n'));
  } finally {
    cleanup(dir);
  }
});
