import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const read = (path: string) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');

test('environment IPC exposes metadata only', () => {
  const commands = read('src-tauri/src/commands/env.rs');
  const invokeRegistry = read('src-tauri/src/lib.rs');
  const host = read('src/lib/tauri/host.ts');
  const types = read('src/lib/tauri/types-api.ts');
  assert.doesNotMatch(commands, /env_get_variables|env_encrypt/);
  assert.doesNotMatch(invokeRegistry, /commands::env::env_get_variables|commands::env::env_encrypt/);
  assert.doesNotMatch(host, /env_get_variables|env_encrypt|getVariables/);
  assert.doesNotMatch(types, /getVariables|encrypt: \(text/);
  assert.match(commands, /Result<Vec<env_manager::EnvProfile>>/);
  assert.match(types, /EnvVariableMetadata/);
});

test('environment settings preserve secret drafts only on failed save', () => {
  const settings = read('src/components/settings/EnvironmentProfilesSettings.tsx');
  const addVariable = settings.slice(
    settings.indexOf('const addVariable = async'),
    settings.indexOf('const deleteVariable = async'),
  );
  const saveIndex = addVariable.indexOf('await api.env.setVariable');
  const clearIndex = addVariable.indexOf("setSecretValue('');");
  const catchIndex = addVariable.indexOf('} catch (cause) {');
  assert.match(settings, /type="password"/);
  assert.match(settings, /autoComplete="new-password"/);
  assert.ok(saveIndex >= 0 && clearIndex > saveIndex && catchIndex > clearIndex);
  assert.doesNotMatch(addVariable.slice(catchIndex), /setSecretValue/);
  assert.match(settings, /open=\{deleteProfileTarget !== null\}/);
  assert.match(settings, /open=\{deleteVariableTarget !== null\}/);
  assert.match(settings, /onConfirm=\{\(\) => void deleteProfile\(\)\}/);
  assert.match(settings, /onConfirm=\{\(\) => void deleteVariable\(\)\}/);
  assert.match(settings, /onClick=\{\(\) => setSelectedProfileId\(profile\.id\)\}[\s\S]*?disabled=\{busy\}/);
  assert.match(settings, /value=\{secretValue\}[\s\S]*?disabled=\{busy\}/);
});

test('environment event consumers filter, coalesce, and unsubscribe', () => {
  const settings = read('src/components/settings/EnvironmentProfilesSettings.tsx');
  const terminal = read('src/components/shell/Terminal.tsx');
  for (const [source, generation] of [
    [settings, 'generation'],
    [terminal, 'profileLoadGeneration'],
  ] as const) {
    assert.match(source, /onDbStateChanged/);
    assert.match(source, /channel !== 'env'/);
    assert.match(source, /clearTimeout\(/);
    assert.match(source, /setTimeout\(/);
    assert.match(source, /unsubscribe\?\.\(\)/);
    assert.match(source, new RegExp(`requestId !== ${generation}\\.current`));
    assert.match(source, new RegExp(`${generation}\\.current \\+= 1`));
    assert.doesNotMatch(source, /setInterval\(/);
  }
});

test('terminal shortcuts create under the selected profile and block load failures', () => {
  const terminal = read('src/components/shell/Terminal.tsx');
  assert.doesNotMatch(terminal, /createTerminalSession\(\);/);
  assert.match(terminal, /createTerminalSession\(undefined, selectedProfileId \?\? undefined\)/);
  assert.match(terminal, /if \(profileLoading \|\| profileLoadError\)/);
  assert.match(terminal, /classifyError\(error, \{ locale \}\)/);
});
