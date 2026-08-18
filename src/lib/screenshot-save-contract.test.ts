import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const read = (path: string) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');

test('screenshot IPC accepts an authorized source and never a Renderer destination', () => {
  const command = read('src-tauri/src/commands/screenshot.rs');
  const host = read('src/lib/tauri/host.ts');
  const types = read('src/lib/tauri/types-api.ts');
  assert.match(command, /SaveAnnotatedRequest/);
  assert.doesNotMatch(command, /target_path|targetPath/);
  assert.match(host, /saveAnnotated: \(request\).*\{ request \}/);
  assert.doesNotMatch(host, /targetPath/);
  assert.match(types, /ScreenshotSaveAnnotatedRequest/);
  assert.doesNotMatch(types, /saveAnnotated: \(dataUrl/);
});

test('screenshot UI preserves context on failure and closes only after success', () => {
  const shell = read('src/components/shell/ShellLayout.tsx');
  const card = read('src/components/screenshot/ScreenshotCard.tsx');
  const editor = read('src/components/screenshot/AnnotationEditor.tsx');
  assert.match(shell, /classifyError\(cause, \{ locale \}\)/);
  assert.match(shell, /saveAnnotated\(\{ sourcePath: annotatingFile, dataUrl \}\)/);
  assert.match(card, /if \(await onSaveToMaterial\(filePath\)\) setVisible\(false\)/);
  assert.match(card, /if \(await onAnnotate\(filePath\)\) setVisible\(false\)/);
  assert.match(editor, /saveState === 'saving'/);
  assert.match(editor, /role=\{saveError \? 'alert' : 'status'\}/);
  assert.doesNotMatch(editor, /💾|↩/);
});
