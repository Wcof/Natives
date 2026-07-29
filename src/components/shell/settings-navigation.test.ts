import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_SETTINGS_VIEW,
  SETTINGS_SECTIONS,
  getSettingsSection,
  isSettingsView,
  normalizeSettingsTarget,
} from './settings-navigation';

test('settings exposes one execution-engine entry and redirects legacy links', () => {
  assert.deepEqual(SETTINGS_SECTIONS, [
    'personal',
    'general',
    'appearance',
    'providers',
    'runtime',
    'plugins',
  ]);
  assert.equal(SETTINGS_SECTIONS.includes('env' as never), false);
  assert.equal(getSettingsSection('settings:engine'), 'runtime');
  assert.equal(getSettingsSection('settings:engineering'), 'runtime');
});

test('settings entry points normalize to general', () => {
  assert.equal(normalizeSettingsTarget('__settings__'), DEFAULT_SETTINGS_VIEW);
  assert.equal(normalizeSettingsTarget('settings'), DEFAULT_SETTINGS_VIEW);
});

test('valid settings targets are preserved', () => {
  assert.equal(
    normalizeSettingsTarget('settings:personal'),
    'settings:personal',
  );
  assert.equal(
    normalizeSettingsTarget('settings:providers'),
    'settings:providers',
  );
  assert.equal(getSettingsSection('settings:runtime'), 'runtime');
});

test('invalid and removed settings targets fall back to general', () => {
  assert.equal(normalizeSettingsTarget('settings:env'), DEFAULT_SETTINGS_VIEW);
  assert.equal(normalizeSettingsTarget('settings:unknown'), DEFAULT_SETTINGS_VIEW);
});

test('ordinary navigation targets are not consumed', () => {
  assert.equal(normalizeSettingsTarget('module:example'), null);
  assert.equal(isSettingsView('dashboard'), false);
});
