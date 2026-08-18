import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import * as React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { classifyError } from '@/lib/error-classifier';
import { resolveDirectoryLoading } from '@/hooks/useFileEntries';
import FileArea, { resolveFileAreaState } from './FileArea';

const browserSource = readFileSync(new URL('../FileBrowser.tsx', import.meta.url), 'utf8');
const hookSource = readFileSync(new URL('../../../hooks/useFileEntries.ts', import.meta.url), 'utf8');
const reactGlobal = globalThis as typeof globalThis & { React: typeof React };
reactGlobal.React = React;

test('directory loading states are mutually exclusive', () => {
  const error = classifyError(new Error('EACCES read failed'), { locale: 'en' });

  assert.equal(resolveFileAreaState(true, null), 'loading');
  assert.equal(resolveFileAreaState(false, error), 'error');
  assert.equal(resolveFileAreaState(false, null), 'success');
  assert.equal(resolveFileAreaState(true, error), 'loading');
});

test('a path change is loading before its effect starts', () => {
  assert.equal(resolveDirectoryLoading(false, '/next', '/previous'), true);
  assert.equal(resolveDirectoryLoading(false, '/next', '/next'), false);
  assert.equal(resolveDirectoryLoading(true, '/next', '/next'), true);
});

test('file read failures retain classified user guidance', () => {
  const error = classifyError(new Error('EACCES read failed'), { locale: 'en' });

  assert.equal(error.category, 'FILE_READ_FAILED');
  assert.ok(error.userMessage);
  assert.ok(error.actionHint);
});

test('retryable failures render classified guidance and retry without the empty state', () => {
  const error = classifyError(new Error('ETIMEDOUT'), { locale: 'en' });
  const noop = () => {};
  const markup = renderToStaticMarkup(React.createElement(FileArea, {
    locale: 'en',
    viewMode: 'grid',
    loading: false,
    error,
    onRetry: noop,
    entries: [],
    isDragging: false,
    dragHandlers: { onDragEnter: noop, onDragLeave: noop, onDragOver: noop, onDrop: noop },
    selectedIndex: -1,
    selectedPaths: new Set<string>(),
    onSelect: noop,
    onItemContextMenu: noop,
    onBlankClick: noop,
    onBlankContextMenu: noop,
    gridSize: 'md',
    sortBy: 'name',
    sortDir: 'asc',
    showDir: false,
    onSort: noop,
    onEditRequest: noop,
    favorites: [],
    onFavoriteToggle: noop,
    onMoveDrop: noop,
    areaRef: { current: null },
    gridContainerRef: { current: null },
    scrollContainerRef: { current: null },
    onViewHandleReady: noop,
  }));

  assert.match(markup, new RegExp(error.userMessage));
  assert.match(markup, new RegExp(error.actionHint));
  assert.match(markup, /<button[^>]*>[\s\S]*?Retry<\/button>/);
  assert.doesNotMatch(markup, /This folder is empty/);
  assert.doesNotMatch(markup, /New File|New Folder|Paste/);
});

test('FileBrowser only renders empty and mutation controls after a successful load', () => {
  assert.match(
    browserSource,
    /const mutationBlocked = entriesHook\.loading \|\| entriesHook\.error !== null;/,
  );
  assert.match(
    browserSource,
    /!mutationBlocked && filteredEntries\.length === 0/,
  );
  assert.match(browserSource, /if \(mutationBlocked\) break;[\s\S]*?ops\.setNewItemTarget/);
  assert.match(browserSource, /!mutationBlocked && contextMenu/);
  assert.match(browserSource, /!mutationBlocked && \(\s*<FileModals/);
  assert.match(browserSource, /open=\{!mutationBlocked && !!ops\.trashTarget\}/);
  assert.match(browserSource, /error=\{entriesHook\.error\}/);
});

test('mutationBlocked is the subscribed authority for header actions and closes stale dialogs', () => {
  const headerEffect = browserSource.match(
    /\/\/ ── Header 动作下行[\s\S]*?useEffect\(\(\) => \{([\s\S]*?)\n {2}\]\);/,
  );
  assert.ok(headerEffect, 'header action effect must exist');
  const headerBody = headerEffect[1];
  assert.ok(headerBody, 'header action effect must have a body');
  assert.match(headerBody, /if \(mutationBlocked\) break;/);
  assert.match(headerBody, /\n {4}mutationBlocked,/);

  assert.match(
    browserSource,
    /if \(!mutationBlocked\) return;[\s\S]*?setContextMenu\(null\);[\s\S]*?ops\.setRenameTarget\(null\);[\s\S]*?ops\.setNewItemTarget\(null\);[\s\S]*?ops\.setTrashTarget\(null\);/,
  );
});

test('stale failures cannot overwrite the latest directory state', () => {
  const catchBlock = hookSource.match(/} catch \(cause\) \{([\s\S]*?)\n[\t ]+} finally/);

  assert.ok(catchBlock, 'loadEntries must retain an explicit catch block');
  const catchBody = catchBlock[1];
  assert.ok(catchBody, 'loadEntries catch block must have a body');
  assert.match(catchBody, /if \(rid !== loadIdRef\.current\) return;/);
  assert.match(
    catchBody,
    /setLoadError\(\{ path: currentPath, classified: classifyError\(cause, \{ locale \}\) \}\)/,
  );
  assert.ok(
    catchBody.indexOf('rid !== loadIdRef.current') < catchBody.indexOf('setLoadError('),
    'generation guard must run before the failure writes state',
  );
});

test('a new load clears persistent error before retrying', () => {
  assert.match(
    hookSource,
    /const rid = \+\+loadIdRef\.current;\n[\t ]+setLoading\(true\);\n[\t ]+setLoadError\(null\);/,
  );
});

test('path changes synchronously hide an error from the previous directory', () => {
  assert.match(
    hookSource,
    /const error = loadError\?\.path === currentPath \? loadError\.classified : null;/,
  );
});
