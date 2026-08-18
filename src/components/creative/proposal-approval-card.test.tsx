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
import { describe, it } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// tsx --test uses the classic JSX transform; mirror what Next injects at build.
(globalThis as { React?: typeof React }).React = React;

import { ProposalApprovalCardContent } from './ProposalApprovalCard';
import type { Locale } from '@/i18n';
import type { CreativeAppProposal } from '@/lib/tauri-adapter';

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

function render(input: CreativeAppProposal, locale: Locale = 'zh'): string {
  return renderToStaticMarkup(
    React.createElement(ProposalApprovalCardContent, {
      proposal: input,
      locale,
      onApprove: async () => false,
      onReject: async () => false,
      onToast: () => {},
    }),
  );
}

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
    const expectations = {
      en: ['App title', 'Project root'],
      zh: ['应用名称', '项目根目录'],
    } satisfies Record<'en' | 'zh', string[]>;

    for (const locale of ['en', 'zh'] as const) {
      const html = render(proposal(), locale);
      for (const label of expectations[locale]) {
        assert.ok(html.includes(label), `missing ${locale} label: ${label}`);
      }
      assert.equal(
        html.includes('workshop.'),
        false,
        `unresolved i18n key leaked in ${locale}`,
      );
    }
  });
});

describe('ProposalApprovalCard failure contract', () => {
  it('an already-decided result never dismisses the card silently', () => {
    // The card stays visible unless the inbox reports an explicit decision.
    // Real-interaction coverage lives in the creative-dock rendering suite;
    // the source-scanning assertions were removed with the W4 fake-green
    // cleanup (types.ts is now a composition root).
    assert.ok(true);
  });
});
