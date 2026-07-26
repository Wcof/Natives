import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { resolvePreviewPhase } from './DraftPreview';
import { IFRAME_SANDBOX } from '../../lib/iframe-manager';
import type { CreativeDraft } from '../../lib/tauri-adapter';

const source = readFileSync(new URL('./DraftPreview.tsx', import.meta.url), 'utf8');

function draft(overrides: Partial<CreativeDraft> = {}): CreativeDraft {
  return {
    draftId: 'd1',
    name: 'demo',
    intent: '做一个番茄钟',
    currentRevision: 1,
    state: 'ready',
    createdAt: '2026-07-26T00:00:00Z',
    updatedAt: '2026-07-26T00:00:00Z',
    ...overrides,
  };
}

test('no draft and no revision resolve to their own empty states', () => {
  assert.equal(
    resolvePreviewPhase({ draft: null, urlState: 'ready', frameLoaded: true }),
    'no-draft',
  );
  assert.equal(
    resolvePreviewPhase({ draft: draft({ currentRevision: 0, state: 'drafting' }), urlState: 'ready', frameLoaded: true }),
    'empty',
  );
});

test('in-flight draft states outrank an unavailable port', () => {
  for (const state of ['generating', 'publishing'] as const) {
    assert.equal(
      resolvePreviewPhase({
        draft: draft({ state, currentRevision: 0 }),
        urlState: 'port-unavailable',
        frameLoaded: false,
      }),
      state,
    );
  }
});

test('port failure and pending load are distinguished, ready needs both', () => {
  assert.equal(
    resolvePreviewPhase({ draft: draft(), urlState: 'port-unavailable', frameLoaded: false }),
    'port-unavailable',
  );
  assert.equal(
    resolvePreviewPhase({ draft: draft(), urlState: 'resolving', frameLoaded: false }),
    'loading',
  );
  assert.equal(
    resolvePreviewPhase({ draft: draft(), urlState: 'ready', frameLoaded: false }),
    'loading',
  );
  assert.equal(
    resolvePreviewPhase({ draft: draft(), urlState: 'ready', frameLoaded: true }),
    'ready',
  );
});

test('preview iframe never grants same-origin access (R-S2)', () => {
  assert.match(source, /sandbox=\{IFRAME_SANDBOX\}/);
  assert.equal(IFRAME_SANDBOX.includes('allow-same-origin'), false);
  // No second sandbox literal may sneak past the shared constant.
  assert.equal(/sandbox=["']/.test(source), false);
});

test('frame identity comes from contentWindow, not the opaque origin', () => {
  assert.match(source, /event\.source !== frame\.contentWindow/);
  const code = source.replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, '');
  assert.equal(/event\.origin/.test(code), false);
});

test('revision rides in the URL so the frame cannot serve a cached revision', () => {
  assert.match(source, /\?rev=\$\{revision\}/);
  assert.match(source, /const frameKey = `\$\{draftId\}:\$\{revision\}`/);
  assert.match(source, /key=\{frameKey\}/);
});
