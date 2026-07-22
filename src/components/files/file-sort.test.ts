import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { nextSortDir, nextSortForField } from './file-sort';

describe('nextSortForField', () => {
  it('toggles direction when the same field is selected (name no-op fix)', () => {
    assert.deepEqual(nextSortForField('name', 'asc', 'name'), {
      sortBy: 'name',
      sortDir: 'desc',
    });
    assert.deepEqual(nextSortForField('name', 'desc', 'name'), {
      sortBy: 'name',
      sortDir: 'asc',
    });
  });

  it('switches to name with ascending default', () => {
    assert.deepEqual(nextSortForField('mtime', 'desc', 'name'), {
      sortBy: 'name',
      sortDir: 'asc',
    });
  });

  it('switches to mtime/size with descending default', () => {
    assert.deepEqual(nextSortForField('name', 'asc', 'mtime'), {
      sortBy: 'mtime',
      sortDir: 'desc',
    });
    assert.deepEqual(nextSortForField('name', 'asc', 'size'), {
      sortBy: 'size',
      sortDir: 'desc',
    });
  });

  it('toggles mtime when re-selected', () => {
    assert.deepEqual(nextSortForField('mtime', 'desc', 'mtime'), {
      sortBy: 'mtime',
      sortDir: 'asc',
    });
  });
});

describe('nextSortDir', () => {
  it('sets explicit direction', () => {
    assert.equal(nextSortDir('asc', 'desc'), 'desc');
    assert.equal(nextSortDir('desc', 'asc'), 'asc');
  });

  it('toggles when value is omitted', () => {
    assert.equal(nextSortDir('asc'), 'desc');
    assert.equal(nextSortDir('desc'), 'asc');
  });

  it('toggles on unknown value', () => {
    assert.equal(nextSortDir('asc', 'nope'), 'desc');
  });
});
