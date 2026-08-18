import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { t } from './index';

const componentKeys = [
  ['../components/files/FindReplaceBar.tsx', [
    'fileBrowser.findInFile',
    'fileBrowser.findPlaceholder',
    'fileBrowser.replacePlaceholder',
    'fileBrowser.findMatchCase',
    'fileBrowser.findPrev',
    'fileBrowser.findNext',
    'fileBrowser.replaceOne',
    'fileBrowser.replaceAll',
  ]],
  ['../components/capabilities/experts/ExpertEditForm.tsx', [
    'settings.engineCapabilities.noToolsAvailable',
    'settings.engineCapabilities.allowTools',
  ]],
  ['../components/assistant/activity-inspector/panels.tsx', ['common.refresh']],
  ['../components/settings/NativeExecutionInspector.tsx', ['fileBrowser.copy']],
  ['../components/files/ImageEditor.tsx', ['imageEditor.textInputDialogLabel']],
] as const;

const invalidKeys = [
  'filePreview.findInFile',
  'filePreview.findPlaceholder',
  'filePreview.replacePlaceholder',
  'filePreview.findMatchCase',
  'filePreview.findPrev',
  'filePreview.findNext',
  'filePreview.replaceOne',
  'filePreview.replaceAll',
  'capabilities.experts.noToolsAvailable',
  'capabilities.experts.allowTools',
  'assistant.activity.refresh',
  'common.copy',
  'common.textInput',
] as const;

test('active file, capability, assistant, and settings UI only references defined i18n keys', () => {
  const sources = componentKeys.map(([path]) =>
    readFileSync(new URL(path, import.meta.url), 'utf8'),
  );
  const combinedSource = sources.join('\n');

  for (const invalidKey of invalidKeys) {
    assert.equal(combinedSource.includes(`'${invalidKey}'`), false, invalidKey);
  }

  for (const [[path, keys], source] of componentKeys.map((entry, index) => [entry, sources[index]] as const)) {
    assert.ok(source, path);
    for (const key of keys) {
      assert.equal(source.includes(`'${key}'`), true, key);
      for (const locale of ['zh', 'en']) {
        assert.notEqual(t(locale, key), key, `${locale}:${key}`);
      }
    }
  }
});
