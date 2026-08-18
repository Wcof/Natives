/**
 * Regression: ConversationTimeline must not mount the full message history
 * (R-P4 MUST). A coarse tail-anchored window caps mounted MessageRow count at
 * TIMELINE_WINDOW_SIZE; older rows are revealed explicitly via "show earlier".
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// The app compiles with the automatic JSX runtime; tsx --test uses the classic
// transform, so component modules without an explicit React import need the
// global. Test-only shim — mirrors what Next injects at build time.
(globalThis as { React?: typeof React }).React = React;

import ConversationTimeline, {
  TIMELINE_WINDOW_SIZE,
  TIMELINE_WINDOW_STEP,
  classifyTimelineDelta,
  timelineWindowStart,
  type Message,
} from './ConversationTimeline';

function makeMessages(count: number): Message[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `msg-${index}`,
    role: index % 2 === 0 ? ('user' as const) : ('assistant' as const),
    contentBlocks: [],
    status: 'completed',
    createdAt: new Date(1700000000000 + index * 1000).toISOString(),
  }));
}

function countRenderedRows(html: string): number {
  return (html.match(/<article/g) ?? []).length;
}

describe('timelineWindowStart (pure)', () => {
  it('renders everything when total fits the window', () => {
    assert.equal(timelineWindowStart(0, 0), 0);
    assert.equal(timelineWindowStart(1, 0), 0);
    assert.equal(timelineWindowStart(TIMELINE_WINDOW_SIZE, 0), 0);
  });

  it('caps mounted rows at the window size for large histories', () => {
    for (const total of [TIMELINE_WINDOW_SIZE + 1, 200, 250, 5000]) {
      const start = timelineWindowStart(total, 0);
      assert.equal(total - start, TIMELINE_WINDOW_SIZE, `total=${total}`);
    }
  });

  it('revealing older rows grows the window without going negative', () => {
    assert.equal(timelineWindowStart(400, TIMELINE_WINDOW_STEP), 400 - TIMELINE_WINDOW_SIZE - TIMELINE_WINDOW_STEP);
    // Over-reveal clamps to 0 (all rows visible).
    assert.equal(timelineWindowStart(200, 10_000), 0);
    // Negative reveal is treated as 0.
    assert.equal(timelineWindowStart(400, -5), 400 - TIMELINE_WINDOW_SIZE);
  });
});

describe('classifyTimelineDelta (pure)', () => {
  const edges = (length: number, firstId: string | null, lastId: string | null) => ({
    length,
    firstId,
    lastId,
  });

  it('first observation keeps the default window', () => {
    assert.equal(classifyTimelineDelta(null, edges(3, 'a', 'c')), 'keep');
  });

  it('streaming append keeps the tail-anchored window', () => {
    assert.equal(
      classifyTimelineDelta(edges(3, 'a', 'c'), edges(4, 'a', 'd')),
      'keep',
    );
    // In-place update (same edges) also keeps.
    assert.equal(
      classifyTimelineDelta(edges(3, 'a', 'c'), edges(3, 'a', 'c')),
      'keep',
    );
  });

  it('host prepend page grows the window so loaded rows stay visible', () => {
    assert.equal(
      classifyTimelineDelta(edges(3, 'b', 'd'), edges(5, 'a', 'd')),
      'prepend',
    );
  });

  it('conversation switch / clear resets local reveals', () => {
    assert.equal(
      classifyTimelineDelta(edges(3, 'a', 'c'), edges(2, 'x', 'y')),
      'reset',
    );
    assert.equal(classifyTimelineDelta(edges(3, 'a', 'c'), edges(0, null, null)), 'reset');
    assert.equal(classifyTimelineDelta(edges(0, null, null), edges(2, 'x', 'y')), 'reset');
  });
});

describe('ConversationTimeline render window (component)', () => {
  it('resolves copy and fork action labels in both locales', () => {
    const message: Message = {
      id: 'user-message',
      role: 'user',
      contentBlocks: [],
      status: 'completed',
      createdAt: new Date(1700000000000).toISOString(),
    };

    const zhHtml = renderToStaticMarkup(
      <ConversationTimeline
        messages={[message]}
        loading={false}
        locale="zh"
        onFork={() => undefined}
      />,
    );
    assert.match(zhHtml, /title="复制" aria-label="复制"/);
    assert.match(zhHtml, /title="从此处派生" aria-label="从此处派生"/);

    const enHtml = renderToStaticMarkup(
      <ConversationTimeline
        messages={[message]}
        loading={false}
        locale="en"
        onFork={() => undefined}
      />,
    );
    assert.match(enHtml, /title="Copy" aria-label="Copy"/);
    assert.match(enHtml, /title="Fork from here" aria-label="Fork from here"/);
    assert.doesNotMatch(`${zhHtml}${enHtml}`, /common\.(copy|fork)/);
  });

  it(`mounts at most ${TIMELINE_WINDOW_SIZE} MessageRow for >200 messages`, () => {
    const total = 250;
    const html = renderToStaticMarkup(
      <ConversationTimeline messages={makeMessages(total)} loading={false} locale="en" />,
    );
    assert.equal(countRenderedRows(html), TIMELINE_WINDOW_SIZE);
    // The hidden remainder is reachable through the "show earlier" button.
    assert.match(html, /data-testid="timeline-show-earlier"/);
    assert.match(html, new RegExp(`${total - TIMELINE_WINDOW_SIZE} hidden`));
  });

  it('renders all rows and no reveal button when history fits the window', () => {
    const html = renderToStaticMarkup(
      <ConversationTimeline messages={makeMessages(40)} loading={false} locale="en" />,
    );
    assert.equal(countRenderedRows(html), 40);
    assert.equal(html.includes('timeline-show-earlier'), false);
  });

  it('local reveal button takes precedence over host paging button', () => {
    const html = renderToStaticMarkup(
      <ConversationTimeline
        messages={makeMessages(250)}
        loading={false}
        locale="en"
        hasMoreOlder
        onLoadOlder={() => undefined}
      />,
    );
    assert.match(html, /data-testid="timeline-show-earlier"/);
    assert.equal(html.includes('Load older messages'), false);
  });
});
