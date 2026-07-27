import assert from 'node:assert/strict';
import test from 'node:test';
import { readActiveProject, writeActiveProject } from './active-project';

test('active project is read from the database and legacy cache is migrated', async () => {
  const writes: Array<[string, unknown]> = [];
  const api = {
    db: {
      get: async () => null,
      set: async (key: string, value: unknown) => { writes.push([key, value]); },
      delete: async () => undefined,
    },
  };
  const result = await readActiveProject(api, '/legacy/project');
  assert.equal(result, '/legacy/project');
  assert.deepEqual(writes, [['active_project_path', '/legacy/project']]);
});

test('database value wins over legacy browser cache', async () => {
  const api = { db: { get: async () => '/db/project', set: async () => undefined, delete: async () => undefined } };
  assert.equal(await readActiveProject(api, '/legacy/project'), '/db/project');
});

test('clearing the active project deletes the database value', async () => {
  let deleted = false;
  const api = { db: { get: async () => null, set: async () => undefined, delete: async () => { deleted = true; } } };
  await writeActiveProject(api, null);
  assert.equal(deleted, true);
});

test('a selected external-volume project remains active until project.list verifies it is gone', async () => {
  let deleted = false;
  const api = {
    db: {
      get: async () => '/Volumes/UNTITLED/project',
      set: async () => undefined,
      delete: async () => { deleted = true; },
    },
  };
  const result = await readActiveProject(api);
  assert.equal(result, '/Volumes/UNTITLED/project');
  assert.equal(deleted, false);
});

test('writing empty string is treated same as null', async () => {
  let deleted = false;
  const api = { db: { get: async () => null, set: async () => undefined, delete: async () => { deleted = true; } } };
  await writeActiveProject(api, '');
  assert.equal(deleted, true);
});
