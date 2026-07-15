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
  // V1.1 uses NEUTRAL_PALETTE values
  assert.equal(theme.primary, '#18181A');
  assert.equal(theme.background, '#FAFAFC');
});

test('should reject invalid hex color', () => {
  assert.throws(() => {
    validateTheme({ ...THEMES.light!, primary: 'not-a-color' });
  });
});

test('light and dark use distinct grayscale brand colors', () => {
  assert.notEqual(THEMES.light!.primary, THEMES.dark!.primary);
  // V1.1: light primary = #18181A (neutral-150), dark primary = #FAFAFC (neutral-950)
  assert.equal(THEMES.light!.primary, '#18181A');
  assert.equal(THEMES.dark!.primary, '#FAFAFC');
});

test('dark theme uses dark surfaces', () => {
  // V1.1: dark background = #010101 (neutral-0), dark surface = #18181A (neutral-150)
  assert.equal(THEMES.dark!.background, '#010101');
  assert.equal(THEMES.dark!.surface, '#18181A');
});

test('light theme uses light surfaces', () => {
  // V1.1: light background = #FAFAFC (neutral-950), light surface = #FFFFFF (neutral-1000)
  assert.equal(THEMES.light!.background, '#FAFAFC');
  assert.equal(THEMES.light!.surface, '#FFFFFF');
});

test('normalizes legacy theme aliases to V1 theme ids', () => {
  assert.equal(normalizeThemeId('terminal-volt'), 'dark');
  assert.equal(normalizeThemeId('frosted-jasmine'), 'light');
  assert.equal(normalizeThemeId('dark'), 'dark');
  assert.equal(normalizeThemeId('light'), 'light');
  // V1.1: new default is dark
  assert.equal(normalizeThemeId('unknown'), 'dark');
});

// applyTheme 需要 DOM 环境，在浏览器集成测试中覆盖

test('chart-volume-0..8 are complete and in correct order', () => {
  const { CHART_VOLUME_DARK, CHART_VOLUME_LIGHT } = require('./theme-engine');
  for (let i = 0; i <= 8; i++) {
    assert.ok(CHART_VOLUME_DARK[i] !== undefined, `dark chart-volume-${i} should exist`);
    assert.ok(CHART_VOLUME_LIGHT[i] !== undefined, `light chart-volume-${i} should exist`);
  }
  // Dark: dark → light as volume increases
  assert.equal(CHART_VOLUME_DARK[0], '#202024');
  assert.equal(CHART_VOLUME_DARK[8], '#FAFAFC');
  // Light: light → dark as volume increases
  assert.equal(CHART_VOLUME_LIGHT[0], '#E8E7EA');
  assert.equal(CHART_VOLUME_LIGHT[8], '#18181A');
});

test('getChartVolumeLevel covers zero and boundary values', () => {
  const { getChartVolumeLevel } = require('./theme-engine');
  // Zero value → level 0
  assert.equal(getChartVolumeLevel(0, 100), 0);
  // Zero visibleMax → level 0
  assert.equal(getChartVolumeLevel(50, 0), 0);
  // Minimum non-zero → level 1
  assert.equal(getChartVolumeLevel(1, 100), 1);
  // Small value → level 1
  assert.equal(getChartVolumeLevel(12, 100), 1);
  // Boundary test: ceil(12.5/100*8) = ceil(1) = 1
  assert.equal(getChartVolumeLevel(12.5, 100), 1);
  // Mid value: ceil(50/100*8) = ceil(4) = 4
  assert.equal(getChartVolumeLevel(50, 100), 4);
  // Near max: ceil(87/100*8) = ceil(6.96) = 7
  assert.equal(getChartVolumeLevel(87, 100), 7);
  // Equal to max → level 8
  assert.equal(getChartVolumeLevel(100, 100), 8);
  // Above max → level 8
  assert.equal(getChartVolumeLevel(200, 100), 8);
});
