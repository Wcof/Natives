import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { validateTheme, THEMES, TERMINAL_THEMES } from './theme-engine';

describe('ThemeEngine', () => {
  it('should have exactly two themes (editorial-index removed)', () => {
    const ids = Object.keys(THEMES);
    assert.equal(ids.length, 2);
    assert.ok(ids.includes('editorial'));
    assert.ok(ids.includes('warm-archive'));
    assert.equal(ids.includes('editorial-index'), false);
  });

  it('should validate a valid theme', () => {
    const theme = validateTheme(THEMES['editorial']!);
    assert.equal(theme.accent, '#ff433d');
    assert.equal(theme.bg, '#f4f1ea');
  });

  it('should reject invalid hex color', () => {
    assert.throws(() => {
      validateTheme({ ...THEMES['editorial'], accent: 'not-a-color' });
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

  it('should have only 2 terminal ANSI themes', () => {
    const ids = Object.keys(TERMINAL_THEMES);
    assert.equal(ids.length, 2);
    assert.equal(ids.includes('editorial-index'), false);
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

  it('should not have editorial-index in the theme registry', () => {
    assert.equal(THEMES['editorial-index'], undefined);
    assert.equal(TERMINAL_THEMES['editorial-index'], undefined);
  });
});
