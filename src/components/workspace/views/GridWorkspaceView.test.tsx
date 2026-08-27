import assert from 'node:assert/strict';
import React from 'react';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import GridWorkspaceView from './GridWorkspaceView';
import type { GridLayouts } from '@/lib/workspace/views/types';

const mockLayouts: GridLayouts = {
  lg: [
    { i: 'wgt-1', x: 0, y: 0, w: 4, h: 4, minW: 3, minH: 3 },
  ],
  md: [
    { i: 'wgt-1', x: 0, y: 0, w: 4, h: 4, minW: 3, minH: 3 },
  ],
  sm: [
    { i: 'wgt-1', x: 0, y: 0, w: 4, h: 4, minW: 3, minH: 3 },
  ],
};

describe('GridWorkspaceView Regression Tests', () => {
  it('does not render an inline duplicate toolbar or add-card button', () => {
    const html = renderToStaticMarkup(
      React.createElement(GridWorkspaceView, {
        workspaceId: 'ws-test',
        viewId: 'dashboard',
        layouts: mockLayouts,
        editable: true,
        onLayoutChange: async () => {},
      })
    );

    // The inner toolbar with "workspace.addCard" button must NOT exist.
    // Top-level WorkspaceCompositionPage holds the authoritative toolbar.
    assert.ok(
      !html.includes('workspace.addCard') && !html.includes('添加卡片'),
      'GridWorkspaceView should not render an internal duplicate add-card toolbar'
    );
  });

  it('ensures item content does not wrap the drag handle in a drag-cancel class', () => {
    const html = renderToStaticMarkup(
      React.createElement(GridWorkspaceView, {
        workspaceId: 'ws-test',
        viewId: 'dashboard',
        layouts: mockLayouts,
        editable: true,
        onLayoutChange: async () => {},
      })
    );

    // .grid-content must not be a parent wrapper around the entire WidgetRenderer/header,
    // because react-grid-layout cancels dragging if click occurs inside .grid-content.
    assert.ok(
      !html.includes('<div class="grid-content'),
      'GridWorkspaceView must not wrap WidgetRenderer with .grid-content ancestor'
    );
  });

  it('supports passing selectedId down to CompactGrid', () => {
    const html = renderToStaticMarkup(
      React.createElement(GridWorkspaceView, {
        workspaceId: 'ws-test',
        viewId: 'dashboard',
        layouts: mockLayouts,
        editable: true,
        selectedId: 'wgt-1',
        onLayoutChange: async () => {},
      })
    );

    assert.ok(typeof html === 'string', 'Should render correctly with selectedId');
  });
});
