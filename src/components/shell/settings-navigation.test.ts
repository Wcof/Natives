import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_SETTINGS_VIEW,
  SETTINGS_SECTIONS,
  getSettingsSection,
  isSettingsView,
  normalizeSettingsTarget,
} from './settings-navigation';

test('settings sections start with the personal overview', () => {
  assert.deepEqual(SETTINGS_SECTIONS, [
    'overview',
    'general',
    'appearance',
    'providers',
    'runtime',
    'engineering',
    'engine',
    'plugins',
  ]);
  assert.equal(SETTINGS_SECTIONS.includes('env' as never), false);
  assert.equal(getSettingsSection('settings:engine'), 'engine');
  assert.equal(getSettingsSection('settings:engineering'), 'engineering');
});

test('settings entry points normalize to the personal overview', () => {
  assert.equal(normalizeSettingsTarget('__settings__'), DEFAULT_SETTINGS_VIEW);
  assert.equal(normalizeSettingsTarget('settings'), DEFAULT_SETTINGS_VIEW);
  assert.equal(DEFAULT_SETTINGS_VIEW, 'settings:overview');
});

test('valid settings targets are preserved', () => {
  assert.equal(
    normalizeSettingsTarget('settings:providers'),
    'settings:providers',
  );
  assert.equal(getSettingsSection('settings:runtime'), 'runtime');
});

test('invalid and removed settings targets fall back to overview', () => {
  assert.equal(normalizeSettingsTarget('settings:env'), DEFAULT_SETTINGS_VIEW);
  assert.equal(normalizeSettingsTarget('settings:unknown'), DEFAULT_SETTINGS_VIEW);
});

test('ordinary navigation targets are not consumed', () => {
  assert.equal(normalizeSettingsTarget('module:example'), null);
  assert.equal(isSettingsView('dashboard'), false);
});
