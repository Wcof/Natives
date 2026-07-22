import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  detectSlashInput,
  listSlashCommands,
} from '../../lib/assistant-slash';

const messageInput = readFileSync(new URL('./MessageInput.tsx', import.meta.url), 'utf8');
const slashPopover = readFileSync(new URL('./SlashCommandPopover.tsx', import.meta.url), 'utf8');
const zh = readFileSync(new URL('../../i18n/zh.ts', import.meta.url), 'utf8');
const en = readFileSync(new URL('../../i18n/en.ts', import.meta.url), 'utf8');

const FAKE_COMMANDS = ['/create-app', '/modify-app', '/list-apps', '/uninstall-app'];

test('slash detection: / and /abc and newline-/abc open; mid-sentence / does not', () => {
  assert.equal(detectSlashInput('/').active, true);
  assert.equal(detectSlashInput('/abc').active, true);
  assert.equal(detectSlashInput('hello\n/abc').active, true);
  assert.equal(detectSlashInput('see path/to/file').active, false);
  assert.equal(detectSlashInput('a / b').active, false);
});

test('no fake hardcoded slash commands in UI or runtime list', () => {
  assert.deepEqual(listSlashCommands(), []);
  for (const id of FAKE_COMMANDS) {
    assert.equal(slashPopover.includes(id), false, `popover still has ${id}`);
    assert.equal(messageInput.includes(id), false, `input still has ${id}`);
  }
  assert.equal(slashPopover.includes('SYSTEM_COMMANDS'), false);
});

test('SlashCommandPopover is presentational — no document keydown listener', () => {
  assert.equal(slashPopover.includes("addEventListener('keydown'"), false);
  assert.equal(slashPopover.includes('addEventListener("keydown"'), false);
  // mousedown outside-close is still allowed
  assert.match(slashPopover, /addEventListener\('mousedown'/);
  assert.match(slashPopover, /bottom-full left-0/);
  assert.match(slashPopover, /data-slash-empty/);
});

test('MessageInput owns slash keyboard on textarea onKeyDown', () => {
  assert.match(messageInput, /handleTextareaKeyDown/);
  assert.match(messageInput, /detectSlashInput/);
  assert.match(messageInput, /listSlashCommands/);
  // Escape closes menu without clearing text
  assert.match(messageInput, /event\.key === 'Escape'/);
  assert.match(messageInput, /closeSlashMenu/);
  // Enter while slash open is preventDefault'd and does not call handleSend(false)
  assert.match(messageInput, /if \(slashOpen\)/);
  // Source: Enter branch under slashOpen prevents default and does not send
  const slashBlock = messageInput.slice(
    messageInput.indexOf('if (slashOpen)'),
    messageInput.indexOf('// Shift+Enter'),
  );
  assert.match(slashBlock, /event\.key === 'Enter'/);
  assert.match(slashBlock, /event\.preventDefault\(\)/);
  assert.equal(slashBlock.includes('handleSend(false)'), false);
  // After menu closed, normal Enter still sends
  assert.match(messageInput, /void handleSend\(false\)/);
  // Shift+Enter newline preserved
  assert.match(messageInput, /event\.key === 'Enter' && event\.shiftKey/);
  // Cmd/Ctrl+Enter force send preserved
  assert.match(messageInput, /event\.metaKey \|\| event\.ctrlKey/);
  assert.match(messageInput, /handleSend\(true\)/);
});

test('slash empty-state copy is localized zh/en', () => {
  assert.match(zh, /slashEmpty:\s*'当前执行引擎未提供可用指令'/);
  assert.match(en, /slashEmpty:\s*'The current runtime does not provide slash commands'/);
  assert.match(messageInput, /assistant\.slashEmpty/);
  assert.match(messageInput, /assistant\.slashHeader/);
});

test('closing menu or sending keeps free-form /text as ordinary message content', () => {
  // No special-case that strips leading slash before send
  assert.equal(/content:\s*input\.replace\(/.test(messageInput), false);
  assert.match(messageInput, /content: input\.trim\(\)/);
  // closeSlashMenu only closes state — does not clear the draft text
  const closeStart = messageInput.indexOf('const closeSlashMenu');
  const closeEnd = messageInput.indexOf('}, []);', closeStart);
  const closeFn = messageInput.slice(closeStart, closeEnd + 6);
  assert.match(closeFn, /setSlashOpen\(false\)/);
  assert.equal(closeFn.includes('setInput'), false);
  assert.equal(closeFn.includes('onDraftChange'), false);
});
