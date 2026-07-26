'use client';

/**
 * Plan Mode approval card.
 *
 * A plan arrives on the permission channel, so without this card it renders in
 * the generic {@link ./PermissionRequestCard} as a `JSON.stringify` blob. That is
 * the wrong shape for the one decision in the product that most deserves to be
 * readable: approving a plan is the moment a run gets its write access back, and
 * a user who skims a JSON dump has not actually approved anything.
 *
 * So the plan is rendered as what it is — a numbered checklist, each step
 * carrying what it does, which files it touches, how risky it is, and whether a
 * checkpoint can undo it. Irreversibility is escalated all the way to the card's
 * own chrome, because it is the single fact that cannot be discovered after the
 * fact.
 *
 * Interaction and style vocabulary are inherited from the permission card:
 * {@link InteractionPromptShell}, the same submit-lock / in-card-error behaviour,
 * Escape to reject, and the same full-width action stack.
 */

import { useCallback, useState } from 'react';
import {
  AlertTriangle,
  Ban,
  CheckCheck,
  CircleHelp,
  ClipboardList,
  FilePenLine,
  Search,
  ShieldAlert,
  Terminal,
  type LucideIcon,
} from 'lucide-react';
import { t } from '@/i18n';
import { InteractionPromptShell } from './InteractionPromptShell';
import {
  PLAN_APPROVAL_SCOPE,
  type Plan,
  type PlanApproval,
  type PlanRisk,
  type PlanStep,
  type PlanStepKind,
} from './plan-approval';

export interface PlanApprovalCardProps {
  /** Permission id to respond against. */
  requestId: string;
  approval: PlanApproval;
  /**
   * May return a Promise. The card awaits it; on failure it unlocks and shows
   * the error in place, exactly like the permission card.
   */
  onApprove: (id: string, scope: string) => void | Promise<void>;
  onReject: (id: string) => void | Promise<void>;
  locale: string;
}

export type PlanApprovalCopy = {
  title: string;
  stepsHeading: string;
  targetsLabel: string;
  irreversibleBadge: string;
  irreversibleStep: string;
  irreversibleBanner: string;
  risksHeading: string;
  questionsHeading: string;
  outOfScopeHeading: string;
  approve: string;
  reject: string;
  processing: string;
  errorFallback: string;
  escapeHint: string;
};

export function planApprovalCopy(locale: string): PlanApprovalCopy {
  return {
    title: t(locale, 'assistant.planApproval.title'),
    stepsHeading: t(locale, 'assistant.planApproval.stepsHeading'),
    targetsLabel: t(locale, 'assistant.planApproval.targetsLabel'),
    irreversibleBadge: t(locale, 'assistant.planApproval.irreversibleBadge'),
    irreversibleStep: t(locale, 'assistant.planApproval.irreversibleStep'),
    irreversibleBanner: t(locale, 'assistant.planApproval.irreversibleBanner'),
    risksHeading: t(locale, 'assistant.planApproval.risksHeading'),
    questionsHeading: t(locale, 'assistant.planApproval.questionsHeading'),
    outOfScopeHeading: t(locale, 'assistant.planApproval.outOfScopeHeading'),
    approve: t(locale, 'assistant.planApproval.approve'),
    reject: t(locale, 'assistant.planApproval.reject'),
    // Shared with the permission card on purpose: these two say nothing
    // plan-specific, and a second wording would drift.
    processing: t(locale, 'assistant.permission.processing'),
    errorFallback: t(locale, 'assistant.permission.errorFallback'),
    escapeHint: t(locale, 'assistant.planApproval.escapeHint'),
  };
}

export function stepCountLabel(locale: string, count: number): string {
  return t(locale, 'assistant.planApproval.stepCount', { count });
}

export function riskLabel(locale: string, risk: PlanRisk): string {
  switch (risk) {
    case 'low':
      return t(locale, 'assistant.planApproval.riskLow');
    case 'medium':
      return t(locale, 'assistant.planApproval.riskMedium');
    case 'high':
      return t(locale, 'assistant.planApproval.riskHigh');
    default: {
      const _exhaustive: never = risk;
      return _exhaustive;
    }
  }
}

export function kindLabel(locale: string, kind: PlanStepKind): string {
  switch (kind) {
    case 'research':
      return t(locale, 'assistant.planApproval.kindResearch');
    case 'edit':
      return t(locale, 'assistant.planApproval.kindEdit');
    case 'command':
      return t(locale, 'assistant.planApproval.kindCommand');
    case 'verify':
      return t(locale, 'assistant.planApproval.kindVerify');
    default: {
      const _exhaustive: never = kind;
      return _exhaustive;
    }
  }
}

const KIND_ICON: Record<PlanStepKind, LucideIcon> = {
  research: Search,
  edit: FilePenLine,
  command: Terminal,
  verify: CheckCheck,
};

/** Risk badge colours. Low is deliberately mute — only medium and high earn ink. */
const RISK_BADGE: Record<PlanRisk, string> = {
  low: 'border-[var(--border-subtle)] text-[var(--text-secondary)]',
  medium: 'border-[var(--warning)]/40 text-[var(--warning)] bg-[var(--warning-soft)]',
  high: 'border-[var(--danger)]/40 text-[var(--danger)] bg-[var(--danger-soft)]',
};

function errorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error && err.message.trim()) return err.message;
  if (typeof err === 'string' && err.trim()) return err;
  return fallback;
}

const BADGE_BASE =
  'inline-flex shrink-0 items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium leading-none';

function Badge({
  className,
  children,
  ...rest
}: { className: string; children: React.ReactNode } & React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span className={`${BADGE_BASE} ${className}`} {...rest}>
      {children}
    </span>
  );
}

function StepRow({
  step,
  index,
  locale,
  copy,
}: {
  step: PlanStep;
  index: number;
  locale: string;
  copy: PlanApprovalCopy;
}) {
  const Icon = KIND_ICON[step.kind];
  return (
    <li
      data-plan-step
      data-plan-step-id={step.id}
      data-plan-step-kind={step.kind}
      data-plan-step-risk={step.risk}
      data-plan-step-irreversible={step.reversible ? 'false' : 'true'}
      className={`flex gap-2.5 border-l-2 py-2 pl-2.5 pr-1 ${
        step.reversible
          ? 'border-l-[var(--border-subtle)]'
          : 'border-l-[var(--danger)]/60 bg-[var(--danger-soft)]/25'
      }`}
    >
      <span
        aria-hidden
        className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--surface)] text-[10px] font-semibold tabular-nums text-[var(--text-secondary)]"
      >
        {index + 1}
      </span>

      <div className="min-w-0 flex-1 space-y-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span
            className="inline-flex shrink-0 items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-[var(--text-secondary)]"
            data-plan-step-kind-label
          >
            <Icon size={11} aria-hidden />
            {kindLabel(locale, step.kind)}
          </span>
          <span className="min-w-0 flex-1 break-words text-xs font-medium text-[var(--text)]">
            {step.title}
          </span>
          <Badge className={RISK_BADGE[step.risk]} data-plan-step-risk-badge>
            {riskLabel(locale, step.risk)}
          </Badge>
          {step.reversible ? null : (
            <Badge
              className="border-[var(--danger)]/50 bg-[var(--danger-soft)] text-[var(--danger)]"
              data-plan-step-irreversible-badge
            >
              <ShieldAlert size={10} aria-hidden />
              {copy.irreversibleStep}
            </Badge>
          )}
        </div>

        {step.detail ? (
          <p className="break-words text-[11px] leading-relaxed text-[var(--text-body)]">
            {step.detail}
          </p>
        ) : null}

        {step.targets.length > 0 ? (
          <div className="flex flex-wrap items-baseline gap-1">
            <span className="text-[10px] text-[var(--text-secondary)]">{copy.targetsLabel}</span>
            {step.targets.map((target) => (
              <code
                key={target}
                data-plan-target
                className="max-w-full break-all rounded border border-[var(--border-subtle)] bg-[var(--surface)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-body)]"
              >
                {target}
              </code>
            ))}
          </div>
        ) : null}
      </div>
    </li>
  );
}

function NoteList({
  heading,
  items,
  icon: Icon,
  tone,
  testAttr,
}: {
  heading: string;
  items: string[];
  icon: LucideIcon;
  tone: string;
  testAttr: string;
}) {
  if (items.length === 0) return null;
  return (
    <section className="space-y-1" {...{ [testAttr]: '' }}>
      <h4 className={`flex items-center gap-1.5 text-[11px] font-semibold ${tone}`}>
        <Icon size={12} aria-hidden />
        {heading}
      </h4>
      <ul className="space-y-0.5 pl-[18px]">
        {items.map((item, i) => (
          <li
            key={`${i}-${item}`}
            className="list-disc break-words text-[11px] leading-relaxed text-[var(--text-body)]"
          >
            {item}
          </li>
        ))}
      </ul>
    </section>
  );
}

function PlanHeading({ plan }: { plan: Plan }) {
  return (
    <div className="space-y-1">
      <p className="break-words text-sm font-semibold leading-snug text-[var(--text)]" data-plan-title>
        {plan.title}
      </p>
      {plan.summary ? (
        <p className="break-words text-xs leading-relaxed text-[var(--text-body)]" data-plan-summary>
          {plan.summary}
        </p>
      ) : null}
    </div>
  );
}

/**
 * Checklist-style approval for a submitted plan.
 *
 * Approval is always one-shot: the daemon pins the scope to `once` regardless of
 * what arrives, so offering the permission card's four scope buttons here would
 * present three choices that do not exist.
 */
export default function PlanApprovalCard({
  requestId,
  approval,
  onApprove,
  onReject,
  locale,
}: PlanApprovalCardProps) {
  const copy = planApprovalCopy(locale);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runAction = useCallback(
    async (action: () => void | Promise<void>) => {
      if (submitting) return;
      setSubmitting(true);
      setError(null);
      try {
        await action();
        // Stay locked on success; the parent drops the pending interaction.
      } catch (err) {
        setError(errorMessage(err, copy.errorFallback));
        setSubmitting(false);
      }
    },
    [submitting, copy.errorFallback],
  );

  const handleApprove = useCallback(() => {
    void runAction(() => onApprove(requestId, PLAN_APPROVAL_SCOPE));
  }, [onApprove, requestId, runAction]);

  const handleReject = useCallback(() => {
    void runAction(() => onReject(requestId));
  }, [onReject, requestId, runAction]);

  const { plan, stepCount, peakRisk, hasIrreversibleStep } = approval;
  // The chrome itself carries the verdict: a plan that cannot be undone, or that
  // peaks at high risk, should not look like a routine confirmation.
  const alarming = hasIrreversibleStep || peakRisk === 'high';

  const focusRing =
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-1 focus-visible:ring-offset-[var(--surface)]';

  return (
    <InteractionPromptShell
      title={copy.title}
      icon={
        <ClipboardList
          size={14}
          className={alarming ? 'text-[var(--danger)]' : 'text-[var(--primary)]'}
        />
      }
      tone={alarming ? 'warning' : 'accent'}
      submitting={submitting}
      processingLabel={copy.processing}
      error={error}
      onEscape={handleReject}
      headerExtra={
        <span className="flex shrink-0 items-center gap-1.5" data-plan-summary-badges>
          <Badge className="border-[var(--border-subtle)] text-[var(--text-secondary)]" data-plan-step-count>
            {stepCountLabel(locale, stepCount)}
          </Badge>
          <Badge className={RISK_BADGE[peakRisk]} data-plan-peak-risk={peakRisk}>
            {riskLabel(locale, peakRisk)}
          </Badge>
        </span>
      }
      data-testid="plan-approval-card"
    >
      <div
        className="space-y-3"
        data-plan-approval-card
        data-submitting={submitting ? 'true' : 'false'}
      >
        <PlanHeading plan={plan} />

        {hasIrreversibleStep ? (
          <p
            role="note"
            data-plan-irreversible-banner
            className="flex items-start gap-2 rounded-lg border border-[var(--danger)]/30 bg-[var(--danger-soft)] px-3 py-2 text-[11px] leading-relaxed text-[var(--danger)]"
          >
            <ShieldAlert size={13} className="mt-px shrink-0" aria-hidden />
            <span>
              <strong className="font-semibold">{copy.irreversibleBadge}</strong>
              {' — '}
              {copy.irreversibleBanner}
            </span>
          </p>
        ) : null}

        <section className="space-y-1.5">
          <h4 className="text-[11px] font-semibold uppercase tracking-wide text-[var(--text-secondary)]">
            {copy.stepsHeading}
          </h4>
          {/* R-P4: a plan is capped at 40 steps, but the card lives over the
              composer — cap the height and scroll rather than push the actions
              off screen. */}
          <ol
            data-plan-steps
            className="max-h-[min(44vh,420px)] divide-y divide-[var(--border-subtle)] overflow-y-auto rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)]/30"
          >
            {plan.steps.map((step, index) => (
              <StepRow key={step.id} step={step} index={index} locale={locale} copy={copy} />
            ))}
          </ol>
        </section>

        <NoteList
          heading={copy.risksHeading}
          items={plan.risks}
          icon={AlertTriangle}
          tone="text-[var(--warning)]"
          testAttr="data-plan-risks"
        />
        <NoteList
          heading={copy.questionsHeading}
          items={plan.openQuestions}
          icon={CircleHelp}
          tone="text-[var(--text-secondary)]"
          testAttr="data-plan-questions"
        />
        <NoteList
          heading={copy.outOfScopeHeading}
          items={plan.outOfScope}
          icon={Ban}
          tone="text-[var(--text-secondary)]"
          testAttr="data-plan-out-of-scope"
        />

        <div
          className="flex w-full flex-col gap-2"
          data-plan-actions
          role="group"
          aria-label={copy.title}
        >
          <button
            type="button"
            data-plan-approve
            disabled={submitting}
            onClick={handleApprove}
            className={`w-full rounded-lg border px-3 py-2 text-left text-xs font-medium transition-all disabled:cursor-not-allowed disabled:opacity-60 ${
              alarming
                ? 'border-[var(--danger)]/40 bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--danger-soft)]'
                : 'border-[var(--primary)]/40 bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)]'
            } ${focusRing}`}
          >
            {copy.approve}
          </button>
          <button
            type="button"
            data-plan-reject
            disabled={submitting}
            onClick={handleReject}
            className={`w-full rounded-lg border border-[var(--border-subtle)] bg-transparent px-3 py-2 text-left text-xs font-medium text-[var(--text-secondary)] transition-all disabled:cursor-not-allowed disabled:opacity-60 hover:bg-[var(--surface-hover)] hover:text-[var(--text)] ${focusRing}`}
          >
            {copy.reject}
          </button>
        </div>

        <p className="sr-only">{copy.escapeHint}</p>
      </div>
    </InteractionPromptShell>
  );
}
