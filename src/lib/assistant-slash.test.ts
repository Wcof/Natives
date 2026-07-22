import assert from 'node:assert/strict';
import test from 'node:test';
import {
  detectSlashInput,
  filterSlashCommands,
  isSlashMenuKey,
  listSlashCommands,
  nextSlashIndex,
  type SlashCommand,
} from './assistant-slash';

test('detectSlashInput: bare / at start is active', () => {
  const d = detectSlashInput('/');
  assert.equal(d.active, true);
  assert.equal(d.query, '');
  assert.equal(d.slashIndex, 0);
});

test('detectSlashInput: /abc at start is active with query', () => {
  const d = detectSlashInput('/abc');
  assert.equal(d.active, true);
  assert.equal(d.query, 'abc');
  assert.equal(d.slashIndex, 0);
});

test('detectSlashInput: /abc after a newline is active', () => {
  const d = detectSlashInput('hello\n/abc');
  assert.equal(d.active, true);
  assert.equal(d.query, 'abc');
  assert.equal(d.slashIndex, 6);
});

test('detectSlashInput: mid-sentence / does not open menu', () => {
  assert.equal(detectSlashInput('see path/to/file').active, false);
  assert.equal(detectSlashInput('a / b').active, false);
  assert.equal(detectSlashInput('foo/bar').active, false);
});

test('detectSlashInput: space after query deactivates', () => {
  assert.equal(detectSlashInput('/abc def').active, false);
  assert.equal(detectSlashInput('/ ').active, false);
});

test('detectSlashInput: respects caret position', () => {
  // Caret before second line slash → inactive
  assert.equal(detectSlashInput('hello\n/abc', 5).active, false);
  // Caret after second-line slash → active
  const d = detectSlashInput('hello\n/abc', 10);
  assert.equal(d.active, true);
  assert.equal(d.query, 'abc');
});

test('listSlashCommands is empty (no fake native commands)', () => {
  assert.deepEqual(listSlashCommands(), []);
  const sourceHints = ['/create-app', '/modify-app', '/list-apps', '/uninstall-app'];
  // Ensure module source does not reintroduce hardcodes via exports
  for (const id of sourceHints) {
    assert.equal(listSlashCommands().some((c) => c.id === id), false);
  }
});

test('filterSlashCommands matches id/label/description', () => {
  const cmds: SlashCommand[] = [
    { id: '/help', label: '/help', description: 'Show help', category: 'system' },
    { id: '/status', label: '/status', description: 'Runtime status', category: 'system' },
  ];
  assert.equal(filterSlashCommands(cmds, '').length, 2);
  assert.equal(filterSlashCommands(cmds, 'help')[0]?.id, '/help');
  assert.equal(filterSlashCommands(cmds, 'runtime')[0]?.id, '/status');
  assert.equal(filterSlashCommands(cmds, 'zzz').length, 0);
});

test('isSlashMenuKey covers Escape/arrows/Enter only', () => {
  assert.equal(isSlashMenuKey('Escape'), true);
  assert.equal(isSlashMenuKey('Enter'), true);
  assert.equal(isSlashMenuKey('ArrowUp'), true);
  assert.equal(isSlashMenuKey('ArrowDown'), true);
  assert.equal(isSlashMenuKey('a'), false);
  assert.equal(isSlashMenuKey('Tab'), false);
});

test('nextSlashIndex wraps around', () => {
  assert.equal(nextSlashIndex(0, 3, 1), 1);
  assert.equal(nextSlashIndex(2, 3, 1), 0);
  assert.equal(nextSlashIndex(0, 3, -1), 2);
  assert.equal(nextSlashIndex(0, 0, 1), 0);
});
