import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  GRID_MARGIN,
  GRID_PADDING,
  GRID_BREAKPOINTS,
  GRID_COLUMNS,
  GRID_ROW_HEIGHT,
} from '@/lib/workspace/views/types';

describe('CompactGrid Layout Constants', () => {
  it('enforces unified 8px grid gap (margin) and 12px outer padding per WSW-3', () => {
    assert.deepEqual(GRID_MARGIN, [8, 8]);
    assert.deepEqual(GRID_PADDING, [12, 12]);
  });

  it('defines standard responsive breakpoints and columns', () => {
    assert.deepEqual(GRID_BREAKPOINTS, { lg: 1200, md: 996, sm: 768 });
    assert.deepEqual(GRID_COLUMNS, { lg: 12, md: 8, sm: 4 });
    assert.equal(GRID_ROW_HEIGHT, 32);
  });
});
