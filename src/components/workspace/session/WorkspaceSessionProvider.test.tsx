import assert from 'node:assert/strict';
import React from 'react';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import { WorkspaceSessionProvider } from './WorkspaceSessionProvider';

describe('WorkspaceSessionProvider Stale Revision Reconciliation Tests', () => {
  it('renders children safely during initial loading state', () => {
    const html = renderToStaticMarkup(
      React.createElement(
        WorkspaceSessionProvider,
        null,
        React.createElement('div', { 'data-testid': 'workspace-child' }, 'Workspace Content')
      )
    );

    assert.ok(html.includes('data-testid="workspace-child"'));
    assert.ok(html.includes('Workspace Content'));
  });
});
