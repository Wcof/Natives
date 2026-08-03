import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_SETTINGS_VIEW,
  SETTINGS_SECTIONS,
  getSettingsSection,
  isSettingsView,
  normalizeSettingsTarget,
} from './settings-navigation';

test('settings starts with personal overview and exposes one execution-engine entry', () => {
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

test('settings entry points normalize to the personal overview', () => {
  assert.equal(normalizeSettingsTarget('__settings__'), DEFAULT_SETTINGS_VIEW);
  assert.equal(normalizeSettingsTarget('settings'), DEFAULT_SETTINGS_VIEW);
  assert.equal(DEFAULT_SETTINGS_VIEW, 'settings:personal');
});

test('valid settings targets are preserved and overview alias is redirected', () => {
  assert.equal(
    normalizeSettingsTarget('settings:personal'),
    'settings:personal',
  );
  assert.equal(
    normalizeSettingsTarget('settings:overview'),
    'settings:personal',
  );
  assert.equal(
    normalizeSettingsTarget('settings:providers'),
    'settings:providers',
  );
  assert.equal(getSettingsSection('settings:runtime'), 'runtime');
});

test('invalid and removed settings targets fall back to personal overview', () => {
  assert.equal(normalizeSettingsTarget('settings:env'), DEFAULT_SETTINGS_VIEW);
  assert.equal(normalizeSettingsTarget('settings:unknown'), DEFAULT_SETTINGS_VIEW);
});

test('ordinary navigation targets are not consumed', () => {
  assert.equal(normalizeSettingsTarget('module:example'), null);
  assert.equal(isSettingsView('dashboard'), false);
});
