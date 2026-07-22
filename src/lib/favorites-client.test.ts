import assert from 'node:assert/strict';
import test from 'node:test';
import {
  normalizeFavorite,
  parseFavorites,
  isFavoritePath,
  favoriteFilePaths,
  toggleFileFavorite,
  removeFavorite,
  favoritesNavTarget,
  makeFavoriteId,
  FAVORITES_MAX_ENTRIES,
} from './favorites-client';

test('normalizeFavorite: legacy string path', () => {
  const item = normalizeFavorite('/Users/me/Projects');
  assert.ok(item);
  assert.equal(item!.kind, 'file');
  assert.equal(item!.target, '/Users/me/Projects');
  assert.equal(item!.label, 'Projects');
  assert.equal(item!.id, 'file:/Users/me/Projects');
});

test('normalizeFavorite: legacy { path, addedAt }', () => {
  const item = normalizeFavorite({ path: '/tmp/notes.md', addedAt: 100 });
  assert.ok(item);
  assert.equal(item!.kind, 'file');
  assert.equal(item!.target, '/tmp/notes.md');
  assert.equal(item!.label, 'notes.md');
  assert.equal(item!.addedAt, 100);
});

test('normalizeFavorite: new { kind, target, label }', () => {
  const item = normalizeFavorite({
    id: 'custom',
    kind: 'module',
    target: 'my-mod',
    label: '我的模块',
    addedAt: 42,
  });
  assert.ok(item);
  assert.equal(item!.id, 'custom');
  assert.equal(item!.kind, 'module');
  assert.equal(item!.label, '我的模块');
});

test('normalizeFavorite: rejects empty / garbage', () => {
  assert.equal(normalizeFavorite(null), null);
  assert.equal(normalizeFavorite(''), null);
  assert.equal(normalizeFavorite({ foo: 1 }), null);
  assert.equal(normalizeFavorite({ kind: 'file' }), null);
});

test('parseFavorites: migrates string[] and dedupes', () => {
  const items = parseFavorites(JSON.stringify(['/a', '/b', '/a']));
  assert.equal(items.length, 2);
  assert.deepEqual(
    items.map((i) => i.target).sort(),
    ['/a', '/b'],
  );
});

test('parseFavorites: sorts by addedAt desc', () => {
  const items = parseFavorites([
    { path: '/old', addedAt: 1 },
    { path: '/new', addedAt: 99 },
    { path: '/mid', addedAt: 50 },
  ]);
  assert.deepEqual(items.map((i) => i.target), ['/new', '/mid', '/old']);
});

test('parseFavorites: mixed legacy + new formats', () => {
  const items = parseFavorites([
    '/legacy-string',
    { path: '/legacy-obj', addedAt: 10 },
    { kind: 'file', target: '/new-fmt', label: 'New', addedAt: 20, isDir: true },
  ]);
  assert.equal(items.length, 3);
  const newFmt = items.find((i) => i.target === '/new-fmt');
  assert.equal(newFmt?.isDir, true);
  assert.equal(newFmt?.label, 'New');
});

test('toggleFileFavorite: add then remove', () => {
  const { next: added, added: wasAdded } = toggleFileFavorite([], '/foo/bar', {
    isDir: true,
    label: 'bar',
  });
  assert.equal(wasAdded, true);
  assert.equal(added.length, 1);
  assert.equal(added[0]!.target, '/foo/bar');
  assert.equal(added[0]!.isDir, true);
  assert.equal(isFavoritePath(added, '/foo/bar'), true);

  const { next: removed, added: wasAdded2 } = toggleFileFavorite(added, '/foo/bar');
  assert.equal(wasAdded2, false);
  assert.equal(removed.length, 0);
  assert.equal(isFavoritePath(removed, '/foo/bar'), false);
});

test('toggleFileFavorite: newest first', () => {
  const a = toggleFileFavorite([], '/a').next;
  // Force older addedAt on first item so second clearly wins sort only via prepend
  const b = toggleFileFavorite(a, '/b').next;
  assert.equal(b[0]!.target, '/b');
  assert.equal(b[1]!.target, '/a');
});

test('removeFavorite by id and by kind+target', () => {
  const base = toggleFileFavorite([], '/x').next;
  const withMod = [
    ...base,
    {
      id: makeFavoriteId('module', 'm1'),
      kind: 'module' as const,
      target: 'm1',
      label: 'M1',
      addedAt: 1,
    },
  ];
  assert.equal(removeFavorite(withMod, { id: base[0]!.id }).length, 1);
  assert.equal(removeFavorite(withMod, { kind: 'module', target: 'm1' }).length, 1);
});

test('favoritesNavTarget maps kinds', () => {
  assert.equal(
    favoritesNavTarget({
      id: 'file:/tmp',
      kind: 'file',
      target: '/tmp',
      label: 'tmp',
      addedAt: 0,
    }),
    '__files__:/tmp',
  );
  assert.equal(
    favoritesNavTarget({
      id: 'module:x',
      kind: 'module',
      target: 'x',
      label: 'x',
      addedAt: 0,
    }),
    'module:x',
  );
  assert.equal(
    favoritesNavTarget({
      id: 'module:module:y',
      kind: 'module',
      target: 'module:y',
      label: 'y',
      addedAt: 0,
    }),
    'module:y',
  );
  assert.equal(
    favoritesNavTarget({
      id: 'view:__assistant__',
      kind: 'view',
      target: '__assistant__',
      label: '助理',
      addedAt: 0,
    }),
    '__assistant__',
  );
});

test('favoriteFilePaths filters non-file kinds', () => {
  const items = parseFavorites([
    { kind: 'file', target: '/f', label: 'f', addedAt: 1 },
    { kind: 'module', target: 'm', label: 'm', addedAt: 2 },
  ]);
  assert.deepEqual(favoriteFilePaths(items), ['/f']);
});

test('parseFavorites caps at MAX', () => {
  const many = Array.from({ length: FAVORITES_MAX_ENTRIES + 20 }, (_, i) => ({
    path: `/p${i}`,
    addedAt: i,
  }));
  assert.equal(parseFavorites(many).length, FAVORITES_MAX_ENTRIES);
});
