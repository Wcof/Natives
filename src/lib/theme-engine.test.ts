import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { validateTheme, THEMES, TERMINAL_THEMES } from './theme-engine';

describe('ThemeEngine', () => {
  it('should have all three themes', () => {
    const ids = Object.keys(THEMES);
    assert.equal(ids.length, 3);
    assert.ok(ids.includes('terminal-volt'));
    assert.ok(ids.includes('warm-archive'));
    assert.ok(ids.includes('editorial-index'));
  });

  it('should validate a valid theme', () => {
    const theme = validateTheme(THEMES['terminal-volt']!);
    assert.equal(theme.accent, '#cdf24b');
    assert.equal(theme.bg, '#0b0c0a');
  });

  it('should reject invalid hex color', () => {
    assert.throws(() => {
      validateTheme({ ...THEMES['terminal-volt'], accent: 'not-a-color' });
    });
  });

  it('should reject missing fields', () => {
    assert.throws(() => {
      validateTheme({ bg: '#000000' } as Record<string, unknown>);
    });
  });

  it('should have terminal ANSI themes for all visual themes', () => {
    for (const themeId of Object.keys(THEMES)) {
      assert.ok(TERMINAL_THEMES[themeId], `Missing terminal theme for ${themeId}`);
    }
  });

  it('should have valid hex colors in all themes', () => {
    const hexRegex = /^#[0-9a-fA-F]{6}$/;
    for (const [id, theme] of Object.entries(THEMES)) {
      for (const [key, value] of Object.entries(theme)) {
        if (key.startsWith('bg') || key.startsWith('text') || key === 'accent' || key === 'accent-ink' || key === 'panel' || key === 'border') {
          assert.ok(hexRegex.test(value as string), `${id}.${key} = ${value} is not a valid hex`);
        }
      }
    }
  });
});
