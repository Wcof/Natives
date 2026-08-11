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
