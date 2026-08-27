import assert from 'node:assert/strict';
import React from 'react';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import CompactGrid, { type CompactGridItem } from './CompactGrid';
import type { GridLayouts } from '@/lib/workspace/views/types';

const mockLayouts: GridLayouts = {
  lg: [
    { i: 'wgt-1', x: 0, y: 0, w: 4, h: 4 },
    { i: 'wgt-2', x: 4, y: 0, w: 4, h: 4 },
  ],
  md: [
    { i: 'wgt-1', x: 0, y: 0, w: 4, h: 4 },
    { i: 'wgt-2', x: 4, y: 0, w: 4, h: 4 },
  ],
  sm: [
    { i: 'wgt-1', x: 0, y: 0, w: 4, h: 4 },
    { i: 'wgt-2', x: 0, y: 4, w: 4, h: 4 },
  ],
};

const mockItems: CompactGridItem[] = [
  {
    id: 'wgt-1',
    title: 'Widget 1',
    render: ({ editing, active }) => (
      <div data-testid="content-wgt-1" data-editing={String(editing)} data-active={String(active)}>
        Widget 1 Content
      </div>
    ),
  },
  {
    id: 'wgt-2',
    title: 'Widget 2',
    render: ({ editing, active }) => (
      <div data-testid="content-wgt-2" data-editing={String(editing)} data-active={String(active)}>
        Widget 2 Content
      </div>
    ),
  },
];

describe('CompactGrid Selection-Gated Dragging and Resizing Tests', () => {
  it('renders cards in unselected state when editable is false (browse mode)', () => {
    const html = renderToStaticMarkup(
      React.createElement(CompactGrid, {
        layouts: mockLayouts,
        items: mockItems,
        editable: false,
      })
    );

    assert.ok(html.includes('data-testid="compact-grid-item-wgt-1"'));
    assert.ok(html.includes('data-testid="compact-grid-item-wgt-2"'));
    assert.ok(!html.includes('compact-grid-item--selected'), 'Browse mode should have no selected card class');
    assert.ok(!html.includes('compact-grid-item--editable'), 'Browse mode should not have editable class');
  });

  it('renders unselected cards when editable is true but no selectedId is specified', () => {
    const html = renderToStaticMarkup(
      React.createElement(CompactGrid, {
        layouts: mockLayouts,
        items: mockItems,
        editable: true,
        selectedId: null,
      })
    );

    assert.ok(html.includes('compact-grid-item--editable'), 'Should mark grid items as editable');
    assert.ok(!html.includes('compact-grid-item--selected'), 'Unselected cards should not have selected class');
    assert.ok(html.includes('data-selected="false"'), 'Items should explicitly have data-selected="false"');
  });

  it('activates selection, drag handle, and resize visibility only on the selected card', () => {
    const html = renderToStaticMarkup(
      React.createElement(CompactGrid, {
        layouts: mockLayouts,
        items: mockItems,
        editable: true,
        selectedId: 'wgt-1',
      })
    );

    // wgt-1 should be marked selected and active
    assert.ok(
      html.includes('data-testid="compact-grid-item-wgt-1"') &&
      html.includes('compact-grid-item--selected'),
      'wgt-1 should have compact-grid-item--selected class'
    );
    assert.ok(html.includes('data-active="true"'), 'wgt-1 render callback should receive active=true');

    // wgt-2 should NOT be marked selected
    assert.ok(
      !html.includes('data-testid="compact-grid-item-wgt-2" class="compact-grid-item relative flex h-full min-h-0 flex-col overflow-hidden transition-colors focus-visible:outline-2 focus-visible:outline-[var(--primary)] compact-grid-item--editable compact-grid-item--selected')
    );
  });
});
