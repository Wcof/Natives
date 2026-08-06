import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  GRANT_KINDS,
  GRANT_POLICIES,
  canRetryAfterGrant,
  eventLabelKey,
  grantHeld,
  grantPolicyFor,
  grantTargetSummary,
  isGrantKind,
  isGrantPolicy,
  kindLabelKey,
  policyLabelKey,
  sortGrantEvents,
  sortGrants,
} from '@/lib/creative-grants';
import type { AppGrant, GrantEvent } from '@/lib/tauri-adapter';

function grant(kind: string, policy: string, path?: string): AppGrant {
  return {
    id: `g-${kind}`,
    applicationId: 'app-1',
    kind,
    policy,
    path: path ?? null,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
  };
}

describe('creative-grants', () => {
  it('exposes the four capability kinds and three policies', () => {
    assert.deepEqual(GRANT_KINDS, ['upload', 'download', 'clipboard', 'window_open']);
    assert.deepEqual(GRANT_POLICIES, ['default_deny', 'one_time', 'persistent']);
  });

  it('defaults everything to deny', () => {
    assert.equal(grantPolicyFor([], 'clipboard'), 'default_deny');
    assert.equal(grantHeld([], 'clipboard'), false);
    assert.equal(grantHeld([grant('clipboard', 'default_deny')], 'clipboard'), false);
  });

  it('holds one_time and persistent grants', () => {
    assert.equal(grantHeld([grant('clipboard', 'one_time')], 'clipboard'), true);
    assert.equal(grantHeld([grant('download', 'persistent')], 'download'), true);
  });

  it('sorts grants by the canonical kind order', () => {
    const grants = sortGrants([
      grant('window_open', 'persistent'),
      grant('clipboard', 'one_time'),
      grant('upload', 'persistent'),
    ]);
    assert.deepEqual(
      grants.map((g) => g.kind),
      ['upload', 'clipboard', 'window_open'],
    );
  });

  it('maps kind/policy/event to i18n keys', () => {
    assert.equal(kindLabelKey('upload'), 'creative.grantKind.upload');
    assert.equal(policyLabelKey('one_time'), 'creative.grantPolicy.one_time');
    assert.equal(eventLabelKey('consumed'), 'creative.grantEvent.consumed');
  });

  it('recognises valid kinds and policies only', () => {
    assert.equal(isGrantKind('download'), true);
    assert.equal(isGrantKind('camera'), false);
    assert.equal(isGrantPolicy('persistent'), true);
    assert.equal(isGrantPolicy('always'), false);
  });

  it('sorts history newest-first by timestamp', () => {
    const events: GrantEvent[] = [
      { ...grant('clipboard', 'one_time'), id: 'e1', event: 'set', createdAt: '2026-01-01T00:00:00Z' },
      { ...grant('clipboard', 'one_time'), id: 'e2', event: 'consumed', createdAt: '2026-01-02T00:00:00Z' },
    ];
    assert.deepEqual(
      sortGrantEvents(events).map((e) => e.id),
      ['e2', 'e1'],
    );
  });

  it('flags retryable capabilities and truncates targets', () => {
    assert.equal(canRetryAfterGrant('download'), true);
    assert.equal(canRetryAfterGrant('window_open'), true);
    assert.equal(canRetryAfterGrant('clipboard'), false);
    const short = 'https://example.com/oauth/callback?code=x';
    assert.equal(grantTargetSummary(short), short);
    assert.equal(grantTargetSummary('x'.repeat(200)).endsWith('…'), true);
  });
});
