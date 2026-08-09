/**
 * Source contracts for shared interaction prompt shell + composer overlay.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const shell = readFileSync(
  resolve(process.cwd(), 'src/components/assistant/InteractionPromptShell.tsx'),
  'utf8',
);
const permission = readFileSync(
  resolve(process.cwd(), 'src/components/assistant/PermissionRequestCard.tsx'),
  'utf8',
);
const askUser = readFileSync(
  resolve(process.cwd(), 'src/components/assistant/AskUserPromptCard.tsx'),
  'utf8',
);
const composer = readFileSync(
  resolve(process.cwd(), 'src/components/assistant/workbench/WorkbenchComposer.tsx'),
  'utf8',
);

test('InteractionPromptShell exports shared composer column class', () => {
  assert.match(shell, /export const COMPOSER_COLUMN_CLASS/);
  assert.match(shell, /max-w-\[860px\]/);
  assert.match(shell, /export function InteractionPromptShell/);
  assert.match(shell, /export function InteractionPromptOverlay/);
  assert.match(shell, /onEscape/);
});

test('PermissionRequestCard is built on InteractionPromptShell', () => {
  assert.match(permission, /from '\.\/InteractionPromptShell'/);
  assert.match(permission, /<InteractionPromptShell/);
  assert.match(permission, /data-permission-scope/);
  assert.match(permission, /once[\s\S]*this_run[\s\S]*project/);
});

test('AskUserPromptCard reuses InteractionPromptShell', () => {
  assert.match(askUser, /from '\.\/InteractionPromptShell'/);
  assert.match(askUser, /<InteractionPromptShell/);
  assert.match(askUser, /onAnswer/);
});

test('composer replaces MessageInput while permission or ask_user is pending', () => {
  assert.match(composer, /composerBlockedByInteraction/);
  assert.match(composer, /data-composer-interaction-overlay/);
  // MessageInput must be behind the interaction gate, not rendered alongside.
  assert.match(
    composer,
    /composerBlockedByInteraction \? \([\s\S]*PermissionRequestCard[\s\S]*AskUserPromptCard[\s\S]*\) : \([\s\S]*<MessageInput/,
  );
  // Must not leave a free-floating permission card above an active MessageInput.
  assert.equal(
    /PermissionRequestCard[\s\S]{0,400}<MessageInput/.test(
      composer.replace(/composerBlockedByInteraction[\s\S]*?<\/MessageInput>/, ''),
    ),
    false,
  );
});
