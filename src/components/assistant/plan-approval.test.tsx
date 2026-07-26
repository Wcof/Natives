/**
 * Plan approval card — payload parsing and checklist rendering.
 *
 * The card is the only place a user is shown what a run is about to be allowed
 * to do, so the tests below care about two things above all: that a plan cannot
 * be made to *look* safer than it is, and that a payload the parser cannot read
 * degrades to the generic permission card instead of blanking the overlay.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// tsx --test uses the classic JSX transform; mirror what Next injects at build.
(globalThis as { React?: typeof React }).React = React;

import {
  EXIT_PLAN_MODE_TOOL,
  MAX_PLAN_STEPS,
  PLAN_APPROVAL_KIND,
  PLAN_APPROVAL_SCOPE,
  parsePlanApproval,
  parsePlanApprovalRequest,
} from './plan-approval';
import PlanApprovalCard from './PlanApprovalCard';

const cardSource = readFileSync(new URL('./PlanApprovalCard.tsx', import.meta.url), 'utf8');
const workbenchSource = readFileSync(new URL('./AssistantWorkbench.tsx', import.meta.url), 'utf8');

/** The envelope the daemon actually publishes (production_tools await_plan_approval). */
function envelope(overrides: Record<string, unknown> = {}) {
  return {
    kind: PLAN_APPROVAL_KIND,
    plan: {
      title: 'Add retry to the uploader',
      summary: 'Wrap the upload call in a bounded retry.',
      steps: [
        { id: 's1', title: 'Read uploader.rs', kind: 'research', targets: [], risk: 'low', reversible: true },
        {
          id: 's2',
          title: 'Add retry loop',
          detail: 'Bounded at three attempts.',
          kind: 'edit',
          targets: ['src/uploader.rs'],
          risk: 'medium',
          reversible: true,
        },
        { id: 's3', title: 'cargo test', kind: 'command', targets: [], risk: 'high', reversible: false },
      ],
      risks: ['Retry could mask a real auth failure'],
      open_questions: ['Max attempts?'],
      out_of_scope: ['Changing the transport'],
    },
    step_count: 3,
    peak_risk: 'high',
    has_irreversible_step: true,
    ...overrides,
  };
}

describe('parsePlanApproval', () => {
  it('reads the daemon envelope in full', () => {
    const parsed = parsePlanApproval(envelope());
    assert.ok(parsed);
    assert.equal(parsed.plan.title, 'Add retry to the uploader');
    assert.equal(parsed.plan.summary, 'Wrap the upload call in a bounded retry.');
    assert.equal(parsed.stepCount, 3);
    assert.equal(parsed.peakRisk, 'high');
    assert.equal(parsed.hasIrreversibleStep, true);
    const editStep = parsed.plan.steps.find((s) => s.id === 's2');
    assert.ok(editStep);
    assert.deepEqual(editStep.targets, ['src/uploader.rs']);
    assert.equal(editStep.detail, 'Bounded at three attempts.');
    assert.deepEqual(parsed.plan.risks, ['Retry could mask a real auth failure']);
    assert.deepEqual(parsed.plan.openQuestions, ['Max attempts?']);
    assert.deepEqual(parsed.plan.outOfScope, ['Changing the transport']);
  });

  it('refuses anything that is not a readable plan', () => {
    const unreadable: unknown[] = [
      null,
      undefined,
      'plan',
      [],
      { kind: 'other', plan: { title: 't', steps: [{ title: 's' }] } },
      { kind: PLAN_APPROVAL_KIND },
      { kind: PLAN_APPROVAL_KIND, plan: { steps: [{ title: 's' }] } }, // no title
      { kind: PLAN_APPROVAL_KIND, plan: { title: 't' } }, // no steps array
      { kind: PLAN_APPROVAL_KIND, plan: { title: 't', steps: [] } },
      { kind: PLAN_APPROVAL_KIND, plan: { title: 't', steps: [{ detail: 'untitled' }] } },
    ];
    for (const input of unreadable) {
      assert.equal(parsePlanApproval(input), null, JSON.stringify(input));
    }
  });

  it('only claims the card for exit_plan_mode', () => {
    assert.equal(parsePlanApprovalRequest('write_file', envelope()), null);
    assert.ok(parsePlanApprovalRequest(EXIT_PLAN_MODE_TOOL, envelope()));
  });

  it('cannot be talked down: the worse of declared and derived wins', () => {
    // A summary claiming calm over steps that are anything but.
    const understated = parsePlanApproval(
      envelope({ peak_risk: 'low', has_irreversible_step: false }),
    );
    assert.ok(understated);
    assert.equal(understated.peakRisk, 'high', 'derived step risk must not be overridden');
    assert.equal(understated.hasIrreversibleStep, true);

    // And the summary can still escalate a plan whose steps look tame.
    const escalated = parsePlanApproval({
      kind: PLAN_APPROVAL_KIND,
      plan: { title: 't', steps: [{ title: 'look around', kind: 'research' }] },
      peak_risk: 'high',
      has_irreversible_step: true,
    });
    assert.ok(escalated);
    assert.equal(escalated.peakRisk, 'high');
    assert.equal(escalated.hasIrreversibleStep, true);
  });

  it('never infers "low and reversible" for a step that writes or runs', () => {
    const parsed = parsePlanApproval({
      kind: PLAN_APPROVAL_KIND,
      plan: {
        title: 't',
        steps: [
          { title: 'read it', kind: 'research' },
          { title: 'patch it', kind: 'edit' },
          { title: 'rm -rf build', kind: 'command' },
        ],
      },
    });
    assert.ok(parsed);
    assert.deepEqual(
      parsed.plan.steps.map((s) => [s.risk, s.reversible]),
      [
        ['low', true],
        ['medium', true],
        ['medium', false],
      ],
    );
    assert.equal(parsed.hasIrreversibleStep, true, 'a bare command step is not undoable');
  });

  it('assigns ids and disambiguates collisions instead of dropping steps', () => {
    const parsed = parsePlanApproval({
      kind: PLAN_APPROVAL_KIND,
      plan: {
        title: 't',
        steps: [{ title: 'one' }, { title: 'two' }, { id: 's1', title: 'three' }],
      },
    });
    assert.ok(parsed);
    assert.equal(parsed.plan.steps.length, 3);
    assert.equal(new Set(parsed.plan.steps.map((s) => s.id)).size, 3);
  });

  it('skips unusable steps but keeps the readable ones', () => {
    const parsed = parsePlanApproval({
      kind: PLAN_APPROVAL_KIND,
      plan: { title: 't', steps: [{ title: 'keep me' }, null, 42, { detail: 'no title' }] },
    });
    assert.ok(parsed);
    assert.deepEqual(
      parsed.plan.steps.map((s) => s.title),
      ['keep me'],
    );
    assert.equal(parsed.stepCount, 1, 'the count must describe what is rendered');
  });

  it('caps steps so a runaway payload cannot flood the overlay', () => {
    const steps = Array.from({ length: MAX_PLAN_STEPS + 25 }, (_, i) => ({ title: `step ${i}` }));
    const parsed = parsePlanApproval({ kind: PLAN_APPROVAL_KIND, plan: { title: 't', steps } });
    assert.ok(parsed);
    assert.equal(parsed.plan.steps.length, MAX_PLAN_STEPS);
  });
});

function render(input: unknown, locale = 'en'): string {
  const approval = parsePlanApproval(input);
  assert.ok(approval);
  return renderToStaticMarkup(
    React.createElement(PlanApprovalCard, {
      requestId: 'perm-1',
      approval,
      locale,
      onApprove: () => {},
      onReject: () => {},
    }),
  );
}

describe('PlanApprovalCard rendering', () => {
  it('renders a checklist, not a JSON dump', () => {
    const html = render(envelope());
    assert.equal(html.includes('&quot;kind&quot;'), false, 'no serialized payload');
    assert.equal((html.match(/data-plan-step="true"/g) ?? []).length, 3);
    for (const title of ['Read uploader.rs', 'Add retry loop', 'cargo test']) {
      assert.ok(html.includes(title), `missing step title: ${title}`);
    }
    // Kind, targets and per-step risk are all on the row.
    assert.ok(html.includes('data-plan-step-kind="command"'));
    assert.ok(html.includes('data-plan-step-risk="high"'));
    assert.ok(html.includes('src/uploader.rs'));
    assert.ok(html.includes('data-plan-target'));
    assert.ok(html.includes('Bounded at three attempts.'));
  });

  it('carries the plan heading, notes and step count', () => {
    const html = render(envelope());
    assert.ok(html.includes('Add retry to the uploader'));
    assert.ok(html.includes('Wrap the upload call in a bounded retry.'));
    assert.ok(html.includes('Retry could mask a real auth failure'));
    assert.ok(html.includes('Max attempts?'));
    assert.ok(html.includes('Changing the transport'));
    assert.ok(html.includes('3 steps'));
    assert.ok(html.includes('data-plan-peak-risk="high"'));
  });

  it('marks irreversible steps in the row, the badge and the banner', () => {
    const html = render(envelope());
    assert.ok(html.includes('data-plan-step-irreversible="true"'));
    assert.ok(html.includes('data-plan-irreversible-banner'));
    assert.ok(html.includes('Cannot be undone'));
  });

  it('stays calm for a fully reversible, low-risk plan', () => {
    const html = render({
      kind: PLAN_APPROVAL_KIND,
      plan: { title: 'Just look around', steps: [{ title: 'read it', kind: 'research' }] },
    });
    assert.equal(html.includes('data-plan-irreversible-banner'), false);
    assert.equal(html.includes('data-plan-step-irreversible="true"'), false);
    assert.ok(html.includes('data-plan-peak-risk="low"'));
    // Empty note sections must not render as bare headings.
    assert.equal(html.includes('data-plan-risks'), false);
    assert.equal(html.includes('data-plan-questions'), false);
    assert.equal(html.includes('data-plan-out-of-scope'), false);
  });

  it('resolves every string through i18n in both locales', () => {
    for (const locale of ['en', 'zh']) {
      const html = render(envelope(), locale);
      assert.equal(
        html.includes('assistant.planApproval.'),
        false,
        `unresolved i18n key leaked in ${locale}`,
      );
      assert.equal(html.includes('assistant.permission.'), false, `unresolved key in ${locale}`);
    }
    assert.ok(render(envelope(), 'zh').includes('不可撤销'));
  });

  it('offers one approval and a rejection — not the permission scope ladder', () => {
    const html = render(envelope());
    assert.ok(html.includes('data-plan-approve'));
    assert.ok(html.includes('data-plan-reject'));
    assert.equal(html.includes('data-permission-scope'), false);
    assert.ok(html.indexOf('data-plan-approve') < html.indexOf('data-plan-reject'));
  });
});

describe('PlanApprovalCard contract', () => {
  it('approves one-shot, matching the scope the daemon pins', () => {
    assert.equal(PLAN_APPROVAL_SCOPE, 'once');
    assert.match(cardSource, /onApprove\(requestId, PLAN_APPROVAL_SCOPE\)/);
  });

  it('reuses the shared interaction shell and its submit-lock behaviour', () => {
    assert.match(cardSource, /from '\.\/InteractionPromptShell'/);
    assert.match(cardSource, /<InteractionPromptShell/);
    assert.match(cardSource, /void \| Promise<void>/);
    assert.match(cardSource, /await action\(\)/);
    assert.match(cardSource, /onEscape=\{handleReject\}/);
    assert.match(cardSource, /disabled=\{submitting\}/);
    assert.match(cardSource, /error=\{error\}/);
    // Lock must survive success — only the failure path unlocks.
    const runAction = cardSource.slice(
      cardSource.indexOf('const runAction = useCallback'),
      cardSource.indexOf('const handleApprove'),
    );
    const tryBlock = runAction.slice(runAction.indexOf('try {'), runAction.indexOf('} catch'));
    assert.equal(tryBlock.includes('setSubmitting(false)'), false);
  });

  it('bounds the step list height rather than pushing the actions off screen', () => {
    assert.match(cardSource, /max-h-\[min\(44vh,420px\)\][^"]*overflow-y-auto/);
  });
});

describe('AssistantWorkbench wiring', () => {
  it('gates the plan card on the RPC it answers with, and falls back to the generic card', () => {
    assert.match(workbenchSource, /hasMethod\(state\.capabilities, 'permission\.respond'\)/);
    assert.match(workbenchSource, /from '@\/lib\/assistant-workspace\/capability-gate'/);
    const branch = workbenchSource.slice(workbenchSource.indexOf('pendingPlanApproval &&'));
    const planIdx = branch.indexOf('<PlanApprovalCard');
    const genericIdx = branch.indexOf('<PermissionRequestCard');
    assert.ok(planIdx > 0 && genericIdx > planIdx, 'generic card is the else branch');
  });

  it('lazy-loads the card but keeps the predicate eager (R-P7)', () => {
    assert.match(workbenchSource, /lazy\(\(\) => import\('\.\/PlanApprovalCard'\)\)/);
    assert.match(workbenchSource, /import \{ parsePlanApprovalRequest.*\} from '\.\/plan-approval'/);
    assert.match(workbenchSource, /<Suspense/);
  });

  it('never reaches the daemon except through the gateway', () => {
    assert.equal(cardSource.includes('nativesAPI'), false);
    assert.equal(cardSource.includes('assistantV2'), false);
  });
});
