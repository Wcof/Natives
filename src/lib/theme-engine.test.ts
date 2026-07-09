import { test } from 'node:test';
import assert from 'node:assert';
import { THEMES, validateTheme, normalizeThemeId } from './theme-engine';

// ── AI Natives V1.0 Theme Engine Tests ──
// V1.0 双主题：light / dark（已废弃 terminal-volt / frosted-jasmine）

test('should have exactly two themes (light / dark)', () => {
  const ids = Object.keys(THEMES);
  assert.equal(ids.length, 2);
  assert.ok(ids.includes('light'));
  assert.ok(ids.includes('dark'));
});

test('should validate a valid theme', () => {
  const theme = validateTheme(THEMES.light!);
  assert.equal(theme.primary, '#FF6B2C');
  assert.equal(theme.background, '#F7F7F5');
});

test('should reject invalid hex color', () => {
  assert.throws(() => {
    validateTheme({ ...THEMES.light!, primary: 'not-a-color' });
  });
});

test('light and dark share the same brand primary', () => {
  assert.equal(THEMES.light!.primary, THEMES.dark!.primary);
  assert.equal(THEMES.light!.primary, '#FF6B2C');
});

test('dark theme uses dark surfaces', () => {
  assert.equal(THEMES.dark!.background, '#0F1115');
  assert.equal(THEMES.dark!.surface, '#171A21');
});

test('light theme uses light surfaces', () => {
  assert.equal(THEMES.light!.background, '#F7F7F5');
  assert.equal(THEMES.light!.surface, '#FFFFFF');
});

test('normalizes legacy theme aliases to V1 theme ids', () => {
  assert.equal(normalizeThemeId('terminal-volt'), 'dark');
  assert.equal(normalizeThemeId('frosted-jasmine'), 'light');
  assert.equal(normalizeThemeId('dark'), 'dark');
  assert.equal(normalizeThemeId('light'), 'light');
  assert.equal(normalizeThemeId('unknown'), 'light');
});

// applyTheme 需要 DOM 环境，在浏览器集成测试中覆盖
