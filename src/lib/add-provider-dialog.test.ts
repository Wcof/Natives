import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

const dialog = readFileSync(new URL('../components/settings/AddProviderDialog.tsx', import.meta.url), 'utf8');

describe('add provider dialog contract', () => {
  it('uses accessible provider options', () => {
    assert.doesNotMatch(dialog, /<div key=\{preset\.name\} onClick=/);
    assert.match(dialog, /<button[\s\S]*className="add-provider-option/);
  });

  it('shows only protocols supported by the backend', () => {
    assert.match(dialog, /CONFIGURABLE_PROVIDER_PRESETS/);
    assert.doesNotMatch(dialog, /selected\.protocol \?\? 'openai_compatible'/);
    assert.match(dialog, /<select[^>]*value=\{selectedProtocol\}/);
    assert.match(dialog, /value="openai_compatible"/);
    assert.match(dialog, /value="anthropic"/);
  });

  it('labels the discover, test, and save sequence', () => {
    assert.match(dialog, /add-provider-steps/);
    assert.match(dialog, /settings\.fetchModels/);
    assert.match(dialog, /assistant\.testConnection/);
  });
});
