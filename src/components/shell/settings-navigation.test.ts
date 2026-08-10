import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_SETTINGS_VIEW,
  SETTINGS_SECTIONS,
  getSettingsSection,
  isSettingsView,
  normalizeSettingsControlId,
  normalizeSettingsTarget,
  resolveSettingsView,
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

test('settings deep links resolve section before the #fragment (W8)', () => {
  assert.equal(getSettingsSection('settings:runtime#engine'), 'runtime');
  assert.equal(getSettingsSection('settings:general#language'), 'general');
  assert.equal(getSettingsSection('settings:overview#usage'), 'personal');
  // Legacy aliases still work with a fragment attached.
  assert.equal(getSettingsSection('settings:engine#paths'), 'runtime');
});

test('control-id fragments are allowlisted and fail safe (W8)', () => {
  assert.equal(normalizeSettingsControlId('general', '#language'), 'language');
  assert.equal(normalizeSettingsControlId('general', 'language'), 'language');
  assert.equal(normalizeSettingsControlId('providers', '#routing'), 'routing');
  // Unknown anchors never run an arbitrary selector.
  assert.equal(normalizeSettingsControlId('general', '#not-a-control'), null);
  assert.equal(normalizeSettingsControlId('personal', '#select-all'), null);
  // Language keywords map to the language control when the section has one.
  assert.equal(normalizeSettingsControlId('general', '#zh'), 'language');
  assert.equal(normalizeSettingsControlId('general', '#en'), 'language');
  assert.equal(normalizeSettingsControlId('providers', '#zh'), null);
});

test('resolveSettingsView returns validated section and control (W8)', () => {
  const deep = resolveSettingsView('settings:general#language');
  assert.deepEqual(deep, { section: 'general', controlId: 'language' });
  const root = resolveSettingsView('settings:providers');
  assert.deepEqual(root, { section: 'providers', controlId: null });
  const unknown = resolveSettingsView('settings:runtime#bogus');
  assert.deepEqual(unknown, { section: 'runtime', controlId: null });
  assert.equal(resolveSettingsView('dashboard'), null);
});
