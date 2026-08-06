/**
 * Creative Dock (T10): real-window actions, no nested interactive controls,
 * full keyboard/ARIA.
 *
 * The dock is a projection of the window snapshot (T07 `windowList`), and its
 * open/focus/minimize/close actions must drive the real Window API — never a
 * local `useState` fake. Each tab follows the tablist pattern: `role="tab"`
 * with a SIBLING close/minimize button (never nested inside the tab button).
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// tsx --test uses the classic JSX transform; mirror what Next injects at build.
(globalThis as { React?: typeof React }).React = React;

import CreativeDock, { type CreativeDockTab } from './CreativeDock';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

const dockSource = readFileSync(new URL('./CreativeDock.tsx', import.meta.url), 'utf8');
const dockHookSource = readFileSync(
  new URL('../../hooks/useCreativeDock.ts', import.meta.url),
  'utf8',
);

/** Detect a <button> nested inside another <button> (invalid interactive nesting). */
function hasNestedButtons(src: string): boolean {
  const re = /<\/?button\b[^>]*>/g;
  let depth = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src)) !== null) {
    const token = m[0];
    if (token.startsWith('</')) {
      depth = Math.max(0, depth - 1);
    } else {
      if (depth >= 1) return true;
      depth += 1;
    }
  }
  return false;
}

function app(partial: Partial<CreativeAppSummary>): CreativeAppSummary {
  return {
    id: 'app-1',
    applicationId: 'application-1',
    source: 'external_github',
    runtime: 'docker_run',
    title: 'Dashboard',
    version: '1.0.0',
    state: 'running',
    actions: { canOpen: true, canStart: false, canStop: true, canDelete: true, canRetry: false },
    ...partial,
  };
}

function tab(partial: Partial<CreativeDockTab>): CreativeDockTab {
  return {
    key: 'win:w-1',
    app: app({}),
    window: {
      id: 'w-1',
      applicationId: 'application-1',
      surfaceId: 'surface-1',
      label: 'creative-window-w-1',
      state: 'open',
      createdAt: 'now',
      updatedAt: 'now',
    },
    state: 'open',
    ...partial,
  };
}

describe('CreativeDock structure (T10)', () => {
  it('never nests an interactive control inside a button', () => {
    assert.equal(
      hasNestedButtons(dockSource),
      false,
      'CreativeDock must not render a <button> inside a <button>',
    );
  });

  it('uses a real tablist/tab pattern with sibling action buttons', () => {
    assert.match(dockSource, /role="tablist"/, 'tablist container role');
    assert.match(dockSource, /role="tab"/, 'tab role on each app entry');
    assert.match(dockSource, /aria-selected=/, 'aria-selected on tabs');
    // Close/minimize are separate sibling buttons with their own accessible names.
    assert.match(dockSource, /workshop\.dockClose/, 'close button label via i18n');
    assert.match(dockSource, /workshop\.dockMinimize/, 'minimize button label via i18n');
  });

  it('drives the real T07 Window API, not local fake state', () => {
    // The dock component only wires tab/close; the actions it forwards must
    // reach the frozen adapter (windowMinimize/Close/Restore/Open).
    assert.match(dockSource, /windowMinimize/, 'minimize → creativeApp.windowMinimize');
    assert.match(dockSource, /windowClose/, 'close → creativeApp.windowClose');
    assert.match(dockSource, /windowRestore/, 'restore → creativeApp.windowRestore');
    assert.match(dockSource, /onMinimize\(tab\)/, 'minimize forwarded to the controller');
    assert.match(dockSource, /onClose\(tab\)/, 'close forwarded to the controller');
    assert.match(dockHookSource, /windowList/, 'snapshot driven by creativeApp.windowList');
    assert.match(dockHookSource, /windowMinimize/, 'hook calls windowMinimize');
    assert.match(dockHookSource, /windowClose/, 'hook calls windowClose');
    assert.match(dockHookSource, /windowRestore/, 'hook calls windowRestore');
    assert.match(dockHookSource, /windowOpen/, 'hook calls windowOpen');
    assert.match(dockHookSource, /getOpenTarget/, 'open resolves the URL via the frozen adapter');
  });

  it('supports arrow-key and Home/End navigation on the tablist', () => {
    assert.match(dockSource, /ArrowRight/, 'ArrowRight moves to the next tab');
    assert.match(dockSource, /ArrowLeft/, 'ArrowLeft moves to the previous tab');
    assert.match(dockSource, /Home/, 'Home moves to the first tab');
    assert.match(dockSource, /End/, 'End moves to the last tab');
    assert.match(dockSource, /\.focus\(\)/, 'moves focus to the target tab button');
  });

  it('renders nothing when there are no tabs', () => {
    const html = renderToStaticMarkup(
      React.createElement(CreativeDock, {
        tabs: [],
        activeKey: null,
        onSelect: () => {},
        onMinimize: () => {},
        onClose: () => {},
      }),
    );
    assert.equal(html, '');
  });
});

describe('CreativeDock rendering (T10)', () => {
  it('renders one tab per open window with aria-selected state', () => {
    const html = renderToStaticMarkup(
      React.createElement(CreativeDock, {
        tabs: [tab({ key: 'win:w-1' }), tab({ key: 'win:w-2', window: { ...tab({}).window!, id: 'w-2' } })],
        activeKey: 'win:w-1',
        onSelect: () => {},
        onMinimize: () => {},
        onClose: () => {},
      }),
    );
    assert.ok(html.includes('role="tablist"'));
    assert.equal(html.match(/role="tab"/g)?.length, 2, 'two tabs');
    assert.ok(html.includes('aria-selected="true"'), 'active tab marked');
    assert.ok(html.includes('aria-selected="false"'), 'inactive tab marked');
    // Every window row exposes a sibling close button with an accessible name.
    assert.ok(html.includes('aria-label'), 'action buttons carry aria-label');
  });

  it('hides the minimize control for minimized windows (restore on select)', () => {
    const minimized = tab({
      state: 'minimized',
      window: { ...tab({}).window!, state: 'minimized' },
    });
    const openTab = tab({ state: 'open' });
    const html = renderToStaticMarkup(
      React.createElement(CreativeDock, {
        tabs: [openTab, minimized],
        activeKey: null,
        onSelect: () => {},
        onMinimize: () => {},
        onClose: () => {},
      }),
    );
    // A running (open) row is tab + minimize + close = 3 controls; a minimized
    // row is tab + close = 2. The minimize button must not render for minimized.
    assert.equal(html.match(/<button/g)?.length ?? 0, 5, '3 open-row + 2 minimized-row controls');
  });

  it('renders a tab for a running app without windows (open action)', () => {
    const html = renderToStaticMarkup(
      React.createElement(CreativeDock, {
        tabs: [tab({ key: 'app:app-1', window: null, state: 'no-window' })],
        activeKey: null,
        onSelect: () => {},
        onMinimize: () => {},
        onClose: () => {},
      }),
    );
    assert.ok(html.includes('Dashboard'), 'app title rendered');
  });
});
