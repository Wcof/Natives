/**
 * Proposal approval card (T06): field contract + full-detail rendering +
 * failure never fakes success.
 *
 * The card is the only place a user is shown what an approved proposal would
 * actually run, so the tests care that it renders the real executable/
 * interpreter, argv, cwd, env KEY names, project root and ownership, and that
 * an approve/reject failure keeps the card visible with the error instead of a
 * success toast.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// tsx --test uses the classic JSX transform; mirror what Next injects at build.
(globalThis as { React?: typeof React }).React = React;

import ProposalApprovalCard from './ProposalApprovalCard';
import type { CreativeAppProposal } from '@/lib/tauri-adapter';

const cardSource = readFileSync(new URL('./ProposalApprovalCard.tsx', import.meta.url), 'utf8');
const inboxSource = readFileSync(new URL('./ProposalInbox.tsx', import.meta.url), 'utf8');
// ARCH-002: type declarations moved from the barrel to the tauri/types module.
const adapterSource = readFileSync(
  new URL('../../lib/tauri/types.ts', import.meta.url),
  'utf8',
);

function proposal(overrides: Partial<CreativeAppProposal> = {}): CreativeAppProposal {
  return {
    proposalId: 'p-1',
    schemaVersion: 1,
    kind: 'create',
    ownership: 'managed',
    title: 'Dashboard',
    projectRoot: '/proj',
    driver: {
      kind: 'python',
      schemaVersion: 1,
      interpreter: '/proj/.venv/bin/python',
      entry: 'app.py',
      args: ['--port', '8080'],
      cwdRelative: '.',
      environmentKeys: ['PORT'],
      port: { mode: 'auto', value: null },
      openPath: '/',
      healthPath: '/',
      startupTimeoutMs: 60_000,
      isVenv: true,
    },
    openPath: '/',
    healthPath: '/',
    environmentKeys: ['PORT'],
    status: 'pending',
    createdAt: 'now',
    updatedAt: 'now',
    runId: 'r-1',
    turnId: 't-1',
    toolCallId: 'tc-1',
    ...overrides,
  };
}

function render(input: CreativeAppProposal): string {
  return renderToStaticMarkup(
    React.createElement(ProposalApprovalCard, {
      proposal: input,
      onApprove: async () => false,
      onReject: async () => false,
      onToast: () => {},
    }),
  );
}

describe('proposal wire field contract (T06)', () => {
  it('the TS proposal type reads environmentKeys, never envKeys', () => {
    // Rust AgentProposal serializes `environmentKeys` (serde camelCase). A
    // `envKeys` twin silently drops the field at runtime — the original bug.
    // (LocalCreativeConfig legitimately keeps its own `envKeys`; the proposal
    // interfaces are what must match the Rust wire.)
    const proposalTypeStart = adapterSource.indexOf('CR-1001/1002: Agent proposal');
    const proposalTypeEnd = adapterSource.indexOf('export interface LaunchPlan');
    const proposalTypeBlock = adapterSource.slice(proposalTypeStart, proposalTypeEnd);
    assert.match(proposalTypeBlock, /environmentKeys: string\[\]/);
    assert.doesNotMatch(
      proposalTypeBlock,
      /\benvKeys\s*:/,
      'proposal type must not use envKeys',
    );
    assert.match(cardSource, /proposal\.environmentKeys/);
    assert.doesNotMatch(cardSource, /\benvKeys\b/, 'card must read environmentKeys');
  });

  it('the inbox keys cards by the stable proposalId', () => {
    assert.match(inboxSource, /proposal\.proposalId/);
    assert.doesNotMatch(inboxSource, /proposalApprove\?\.\(proposal\)/, 'approve must take the id');
  });
});

describe('ProposalApprovalCard rendering', () => {
  it('renders the full executable, argv, cwd, env keys and project root', () => {
    const html = render(proposal());
    assert.ok(html.includes('/proj/.venv/bin/python'), 'interpreter path');
    assert.ok(html.includes('app.py --port 8080'), 'entry + args');
    assert.ok(html.includes('PORT'), 'env key name');
    assert.ok(html.includes('/proj'), 'project root');
    assert.ok(html.includes('managed'), 'ownership');
    assert.ok(html.includes('data-proposal-executable'));
    assert.ok(html.includes('data-proposal-args'));
    assert.ok(html.includes('data-proposal-env-keys'));
  });

  it('renders a binary executable path and its argv', () => {
    const html = render(
      proposal({
        driver: {
          kind: 'binary',
          schemaVersion: 1,
          executablePath: '/proj/target/release/app',
          executableHash: '',
          approved: true,
          args: ['--serve'],
          cwdRelative: '.',
          environmentKeys: [],
          port: { mode: 'auto', value: null },
          openPath: '/',
          healthPath: '/',
          startupTimeoutMs: 60_000,
        },
        environmentKeys: [],
      }),
    );
    assert.ok(html.includes('/proj/target/release/app'));
    assert.ok(html.includes('--serve'));
    assert.ok(html.includes('data-proposal-executable'));
  });

  it('resolves every string through i18n in both locales', () => {
    for (const locale of ['en', 'zh'] as const) {
      const html = render(proposal());
      assert.equal(
        html.includes('workshop.proposal'),
        false,
        `unresolved i18n key leaked in ${locale}`,
      );
    }
  });
});

describe('ProposalApprovalCard failure contract', () => {
  it('the approve catch path shows the error and never toasts success', () => {
    const approveStart = cardSource.indexOf('const handleApprove');
    const approveEnd = cardSource.indexOf('const handleReject');
    const approveBody = cardSource.slice(approveStart, approveEnd);
    // Success toast only in the try block.
    const tryBlock = approveBody.slice(
      approveBody.indexOf('try {'),
      approveBody.indexOf('} catch'),
    );
    assert.match(tryBlock, /workshop\.proposalApproved/);
    // The catch block sets the error and toasts the classified message — it
    // must not contain the success key.
    const catchBlock = approveBody.slice(approveBody.indexOf('} catch'), approveBody.indexOf('} finally'));
    assert.match(catchBlock, /setError\(classified\.userMessage\)/);
    assert.doesNotMatch(catchBlock, /proposalApproved/, 'no success toast in the catch path');
  });

  it('the inbox rethrows so the card stays visible with the error', () => {
    // T06 bug #3: ProposalInbox swallowed approve/reject errors (caught and
    // toasted), so the card followed up with a success toast. The error must
    // propagate to the card — the inbox must not catch it at all.
    assert.doesNotMatch(
      inboxSource,
      /catch \(err\) \{\s*onToast\(/,
      'inbox must not swallow the error into a toast',
    );
    const approveBody = inboxSource.slice(
      inboxSource.indexOf('const approve'),
      inboxSource.indexOf('const reject'),
    );
    assert.equal(approveBody.includes('catch (err)'), false, 'approve must not catch errors');
  });

  it('an already-decided result never dismisses the card silently', () => {
    // The inbox only dismisses on an explicit `approved`/`rejected` result.
    assert.match(inboxSource, /result\?\.status === 'approved'/);
    assert.match(inboxSource, /result\?\.status === 'rejected'/);
  });

  it('a duplicate event (already_decided) never resolves as a fresh success', () => {
    // Repeat clicks / a decision made in another window return `already_decided`
    // — an idempotent no-op. The inbox must signal "no state change" (false) so
    // the card skips its success toast. Only an explicit `approved` is true.
    const approveBody = inboxSource.slice(
      inboxSource.indexOf('const approve'),
      inboxSource.indexOf('const reject'),
    );
    assert.match(approveBody, /status === 'approved'/);
    const decisionReturn = approveBody.slice(
      approveBody.indexOf("status === 'approved'"),
      approveBody.indexOf('} finally'),
    );
    assert.match(decisionReturn, /return true/, 'approved resolves true');
    assert.match(
      decisionReturn,
      /return false/,
      'already_decided resolves false (no success toast)',
    );

    // The card only toasts success when the decision actually changed.
    const cardApproveStart = cardSource.indexOf('const handleApprove');
    const cardApproveEnd = cardSource.indexOf('const handleReject');
    const cardApproveBody = cardSource.slice(cardApproveStart, cardApproveEnd);
    assert.match(
      cardApproveBody,
      /if \(changed\) onToast\(/,
      'success toast gated on a real state change',
    );
  });
});
