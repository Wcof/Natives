import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const source = readFileSync(new URL('./ReleaseWizardDialog.tsx', import.meta.url), 'utf8');

test('release wizard only marks version preparation successful after the Host resolves', () => {
  const prepare = source.match(/if \(s\.action === 'update-version'\) \{([\s\S]*?)continue;/);
  const preparation = prepare?.[1];
  assert.ok(preparation);
  assert.match(preparation, /await api\.release\.prepare\(projectPath\.trim\(\), newVersion\.trim\(\)\);/);
  assert.match(preparation, /\[s\.action\]: 'ok'/);
  assert.ok(preparation.indexOf('await api.release.prepare') < preparation.indexOf("[s.action]: 'ok'"));
});

test('release wizard retries only unfinished allowlisted actions', () => {
  assert.match(source, /if \(stepStatus\[s\.action\] === 'ok'\) continue;/);
  assert.match(source, /api\.release\.execute\(projectPath\.trim\(\), newVersion\.trim\(\), s\.action\)/);
  assert.doesNotMatch(source, /s\.command|release\.execute\(projectPath\.trim\(\), s\./);
});
