/**
 * Plan approval payload — wire parsing for the Plan Mode approval card.
 *
 * The daemon submits a plan for approval as an ordinary `PermissionRequested`
 * event whose `tool_name` is `exit_plan_mode` and whose `input` is the card
 * envelope below. Reusing the permission channel is deliberate on the engine
 * side (the existing respond RPC and restart-recovery path work unchanged), so
 * telling the two apart is the GUI's job — that is what {@link isPlanApprovalRequest}
 * is for.
 *
 * This module is intentionally free of JSX so the *predicate* can be evaluated
 * eagerly while the card component itself stays lazily loaded (R-P7).
 *
 * Everything here degrades rather than throws. A payload this file cannot make
 * sense of returns `null`, and the caller falls back to the generic permission
 * card — which shows the raw JSON. That is worse to read but still answerable,
 * and a user who cannot answer a pending approval has a wedged run.
 */

/** `input.kind` discriminator written by the daemon. */
export const PLAN_APPROVAL_KIND = 'plan_approval';

/** Tool name carried by a plan approval request. Must match `plan_mode::EXIT_PLAN_MODE_TOOL`. */
export const EXIT_PLAN_MODE_TOOL = 'exit_plan_mode';

/**
 * Scope sent when approving a plan.
 *
 * The daemon overwrites whatever scope arrives with `once` — a plan is approved
 * as a single artifact and there is no such thing as "always approve plans".
 * Sending `once` keeps the wire honest about what actually happens.
 */
export const PLAN_APPROVAL_SCOPE = 'once';

/** Mirrors `plan_mode::MAX_PLAN_STEPS`. Anything beyond is a malformed payload. */
export const MAX_PLAN_STEPS = 40;

/** Per-step target cap. Only guards against a pathological payload. */
const MAX_TARGETS_PER_STEP = 50;

/** Cap on each of the risks / questions / non-goals lists. */
const MAX_LIST_ITEMS = 40;

export type PlanStepKind = 'research' | 'edit' | 'command' | 'verify';
export type PlanRisk = 'low' | 'medium' | 'high';

/** Severity order, so "the worse of two sources" is expressible. */
export const RISK_SEVERITY: Record<PlanRisk, number> = { low: 0, medium: 1, high: 2 };

export interface PlanStep {
  id: string;
  title: string;
  detail: string;
  kind: PlanStepKind;
  targets: string[];
  risk: PlanRisk;
  reversible: boolean;
}

export interface Plan {
  title: string;
  summary: string;
  steps: PlanStep[];
  risks: string[];
  openQuestions: string[];
  outOfScope: string[];
}

export interface PlanApproval {
  plan: Plan;
  /** Number of steps actually rendered. */
  stepCount: number;
  peakRisk: PlanRisk;
  hasIrreversibleStep: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function str(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function stringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const item of value) {
    const s = str(item);
    if (s) out.push(s);
    if (out.length >= MAX_LIST_ITEMS) break;
  }
  return out;
}

function parseKind(value: unknown): PlanStepKind {
  switch (str(value).toLowerCase()) {
    case 'edit':
      return 'edit';
    case 'command':
      return 'command';
    case 'verify':
      return 'verify';
    default:
      return 'research';
  }
}

function parseRisk(value: unknown): PlanRisk | null {
  switch (str(value).toLowerCase()) {
    case 'high':
      return 'high';
    case 'medium':
      return 'medium';
    case 'low':
      return 'low';
    default:
      return null;
  }
}

/**
 * Risk when the payload does not state one.
 *
 * Mirrors the daemon's inference: a step that writes or runs something is never
 * "low risk by omission". A GUI that guessed `low` here would quietly present
 * the most dangerous shape of a malformed plan as the safest.
 */
function riskFromKind(kind: PlanStepKind): PlanRisk {
  return kind === 'edit' || kind === 'command' ? 'medium' : 'low';
}

function worse(a: PlanRisk, b: PlanRisk): PlanRisk {
  return RISK_SEVERITY[a] >= RISK_SEVERITY[b] ? a : b;
}

function parseStep(raw: unknown, index: number, usedIds: Set<string>): PlanStep | null {
  if (!isRecord(raw)) return null;
  const title = str(raw.title);
  if (!title) return null;

  const kind = parseKind(raw.kind);

  let id = str(raw.id) || `s${index + 1}`;
  // The daemon rejects duplicate ids, so a collision here means a payload that
  // did not come from it. Disambiguate rather than drop: a React key clash is
  // ours to solve, and silently hiding a step from an approval card is not.
  if (usedIds.has(id)) id = `${id}-${index + 1}`;
  usedIds.add(id);

  const targets = Array.isArray(raw.targets)
    ? raw.targets
        .map(str)
        .filter((s): s is string => s.length > 0)
        .slice(0, MAX_TARGETS_PER_STEP)
    : [];

  return {
    id,
    title,
    detail: str(raw.detail),
    kind,
    targets,
    risk: parseRisk(raw.risk) ?? riskFromKind(kind),
    // Only file edits are covered by the checkpoint system. Absent a stated
    // value, assume the step cannot be undone.
    reversible:
      typeof raw.reversible === 'boolean'
        ? raw.reversible
        : kind === 'edit' || kind === 'research' || kind === 'verify',
  };
}

/**
 * Parse a `PermissionRequested.input` envelope into a renderable approval.
 *
 * Returns `null` for anything that is not a plan approval, or for a plan with
 * no title or no usable step — a checklist with nothing on it is not something
 * a person can meaningfully approve, and rendering one would invite a click.
 *
 * `peakRisk` and `hasIrreversibleStep` are taken as the **worse** of the
 * daemon-supplied summary and the value recomputed from the steps. The two
 * should agree; when they do not, the summary is the field an attacker or a bug
 * would use to make a dangerous plan look calm, so the pessimistic reading wins.
 */
export function parsePlanApproval(input: unknown): PlanApproval | null {
  if (!isRecord(input)) return null;
  if (str(input.kind) !== PLAN_APPROVAL_KIND) return null;

  const rawPlan = isRecord(input.plan) ? input.plan : null;
  if (!rawPlan) return null;

  const title = str(rawPlan.title);
  if (!title) return null;

  if (!Array.isArray(rawPlan.steps)) return null;
  const usedIds = new Set<string>();
  const steps: PlanStep[] = [];
  for (const raw of rawPlan.steps.slice(0, MAX_PLAN_STEPS)) {
    const step = parseStep(raw, steps.length, usedIds);
    if (step) steps.push(step);
  }
  if (steps.length === 0) return null;

  const derivedPeak = steps.map((s) => s.risk).reduce(worse, 'low' as PlanRisk);
  const declaredPeak = parseRisk(input.peak_risk);
  const derivedIrreversible = steps.some((s) => !s.reversible);

  return {
    plan: {
      title,
      summary: str(rawPlan.summary),
      steps,
      risks: stringList(rawPlan.risks),
      openQuestions: stringList(rawPlan.open_questions),
      outOfScope: stringList(rawPlan.out_of_scope),
    },
    stepCount: steps.length,
    peakRisk: declaredPeak ? worse(derivedPeak, declaredPeak) : derivedPeak,
    hasIrreversibleStep: derivedIrreversible || input.has_irreversible_step === true,
  };
}

/**
 * Parse a pending permission request as a plan approval, or `null` if it is not
 * one. Single entry point: callers need the parsed plan, not just a yes/no, so a
 * separate predicate would only invite parsing twice.
 *
 * Both checks matter. The tool name alone would misfire on a future
 * `exit_plan_mode` payload shaped differently; the `kind` alone would let any
 * tool claim the plan card and the trust that comes with it.
 */
export function parsePlanApprovalRequest(toolName: string, input: unknown): PlanApproval | null {
  if (toolName !== EXIT_PLAN_MODE_TOOL) return null;
  return parsePlanApproval(input);
}
