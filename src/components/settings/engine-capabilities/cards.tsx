'use client';

import type { ReactNode } from 'react';
import { Activity, FileText, Loader, Puzzle, Wrench } from 'lucide-react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import type { DaemonCapabilities } from '@/lib/assistant-protocol';
import { SPACING } from '@/lib/design-tokens';
import type { ExtensionAdminSnapshot, EngineRateLimitSnapshot } from '@/lib/assistant-workspace/capability-admin';
import {
  EXTENSION_DISCOVERY_STATUS,
  type EngineCapabilityRun,
  type Loadable,
} from './model';

export function AvailabilityCard({
  locale,
  state,
  onRetry,
}: {
  locale: Locale;
  state: Loadable<DaemonCapabilities>;
  onRetry: () => void;
}) {
  return (
    <div className="settings-section-card mb-3">
      <div className="settings-section-heading settings-section-heading-with-icon">
        <span className="settings-preference-icon">
          <Activity size={16} />
        </span>
        <div>
          <h4>{t(locale, 'settings.engineCapabilities.availabilityTitle')}</h4>
          <p className="text-xs text-[var(--text-muted)]">
            {t(locale, 'settings.engineCapabilities.availabilityDesc')}
          </p>
        </div>
      </div>
      {state.phase === 'loading' || state.phase === 'idle' ? (
        <Loading locale={locale} />
      ) : state.phase === 'error' ? (
        <SectionError locale={locale} message={state.message} onRetry={onRetry} />
      ) : state.phase === 'unavailable' ? (
        <Unavailable locale={locale} />
      ) : (
        <div style={{ padding: SPACING.md }}>
          <div className="mb-3 grid gap-2 text-xs md:grid-cols-3">
            <DataCell
              label={t(locale, 'settings.engineCapabilities.protocolVersion')}
              value={state.data.protocolVersion}
            />
            <DataCell
              label={t(locale, 'settings.engineCapabilities.advertisedMethods')}
              value={String(state.data.methods.length)}
            />
            <DataCell
              label={t(locale, 'settings.engineCapabilities.providers')}
              value={state.data.providers.join(', ') || '—'}
            />
          </div>
          <div className="flex flex-wrap gap-2">
            <AvailabilityFlag locale={locale} labelKey="tools" available={state.data.tools} />
            <AvailabilityFlag locale={locale} labelKey="hooks" available={state.data.hooks} />
            <AvailabilityFlag
              locale={locale}
              labelKey="subagents"
              available={state.data.subagents}
            />
            <AvailabilityFlag locale={locale} labelKey="mcp" available={state.data.mcp} />
            <AvailabilityFlag
              locale={locale}
              labelKey="extensions"
              available={state.data.extensions}
            />
            <AvailabilityFlag
              locale={locale}
              labelKey="scheduler"
              available={state.data.scheduler}
            />
          </div>
          {(state.data.runtimes ?? []).length > 0 ? (
            <div className="mt-3 space-y-2">
              {(state.data.runtimes ?? []).map((runtime) => (
                <div
                  key={runtime.id}
                  className="flex flex-wrap items-center justify-between gap-2 rounded border border-[var(--border)] p-2 text-xs"
                >
                  <strong>{runtime.displayName}</strong>
                  <span>{runtime.status}</span>
                </div>
              ))}
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}

export function RunEvidenceCard({
  locale,
  run,
  snapshotLoaded,
  selectedSkillIds,
  selectedMcpIds,
  selectedExpertId,
  selectedTeamId,
}: {
  locale: Locale;
  run: EngineCapabilityRun;
  snapshotLoaded: boolean;
  selectedSkillIds: string[];
  selectedMcpIds: string[];
  selectedExpertId: string | null;
  selectedTeamId: string | null;
}) {
  return (
    <div className="settings-section-card mb-3" data-testid="engine-run-evidence">
      <div className="settings-section-heading settings-section-heading-with-icon">
        <span className="settings-preference-icon">
          <Activity size={16} />
        </span>
        <div>
          <h4>{t(locale, 'settings.engineCapabilities.runEvidenceTitle')}</h4>
          <p className="text-xs text-[var(--text-muted)]">
            {t(locale, 'settings.engineCapabilities.runEvidenceDesc')}
          </p>
        </div>
      </div>
      <div style={{ padding: SPACING.md }}>
        <div className="grid gap-2 text-xs md:grid-cols-3">
          <DataCell label={t(locale, 'settings.engineCapabilities.runId')} value={run.id} />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.status')}
            value={run.status}
          />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.provider')}
            value={run.provider_id}
          />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.model')}
            value={run.model_id}
          />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.runtime')}
            value={run.runtime_id ?? 'native'}
          />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.permission')}
            value={run.permission_profile}
          />
        </div>
        <div className="mt-3 flex flex-wrap gap-2">
          <StatusPill
            label={
              snapshotLoaded
                ? t(locale, 'settings.engineCapabilities.loaded')
                : t(locale, 'settings.engineCapabilities.notLoaded')
            }
            tone={snapshotLoaded ? 'success' : 'warning'}
          />
        </div>
        <div className="mt-3 grid gap-2 text-xs md:grid-cols-2">
          <DataCell
            label={t(locale, 'settings.engineCapabilities.selectedSkills')}
            value={selectedSkillIds.join(', ') || '—'}
          />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.selectedMcp')}
            value={selectedMcpIds.join(', ') || '—'}
          />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.selectedExpert')}
            value={selectedExpertId ?? '—'}
          />
          <DataCell
            label={t(locale, 'settings.engineCapabilities.selectedTeam')}
            value={selectedTeamId ?? '—'}
          />
        </div>
      </div>
    </div>
  );
}

export function LoadableSection<T>({
  icon,
  title,
  description,
  state,
  count,
  empty,
  locale,
  onRetry,
  render,
}: {
  icon: ReactNode;
  title: string;
  description?: string;
  state: Loadable<T>;
  count?: (data: T) => number;
  empty: string;
  locale: Locale;
  onRetry: () => void;
  render: (data: T) => ReactNode;
}) {
  const itemCount = state.phase === 'success' && count ? count(state.data) : null;
  return (
    <div className="settings-section-card mb-3">
      <div className="settings-section-heading settings-section-heading-with-icon">
        <span className="settings-preference-icon">{icon}</span>
        <div>
          <h4>
            {title}
            {itemCount !== null ? (
              <span className="ml-1 text-xs font-normal text-[var(--text-muted)]">
                ({itemCount})
              </span>
            ) : null}
          </h4>
          {description ? (
            <p className="text-xs text-[var(--text-muted)]">{description}</p>
          ) : null}
        </div>
      </div>
      {state.phase === 'idle' || state.phase === 'loading' ? (
        <Loading locale={locale} />
      ) : state.phase === 'error' ? (
        <SectionError locale={locale} message={state.message} onRetry={onRetry} />
      ) : state.phase === 'unavailable' ? (
        <Unavailable locale={locale} />
      ) : itemCount === 0 ? (
        <div className="settings-plugin-empty" style={{ padding: SPACING.md }}>
          {empty}
        </div>
      ) : (
        render(state.data)
      )}
    </div>
  );
}

export function EvidenceCard({
  icon,
  title,
  loaded,
  empty,
  emptyLabel,
  loadedLabel,
  missingLabel,
  children,
}: {
  icon: ReactNode;
  title: string;
  loaded: boolean;
  empty: boolean;
  emptyLabel: string;
  loadedLabel: string;
  missingLabel: string;
  children: ReactNode;
}) {
  return (
    <div className="settings-section-card mb-3">
      <div className="settings-section-heading settings-section-heading-with-icon">
        <span className="settings-preference-icon">{icon}</span>
        <div className="flex flex-wrap items-center gap-2">
          <h4>{title}</h4>
          <StatusPill
            label={loaded ? loadedLabel : missingLabel}
            tone={loaded ? 'success' : 'warning'}
          />
        </div>
      </div>
      <div className="space-y-2" style={{ padding: SPACING.md }}>
        {empty ? <div className="settings-plugin-empty">{emptyLabel}</div> : children}
      </div>
    </div>
  );
}

export function InventoryList({ children }: { children: ReactNode }) {
  return (
    <ul className="settings-plugin-list m-0 list-none p-0">
      {children}
    </ul>
  );
}

export function InventoryRow({
  title,
  detail,
  locale,
  selected,
  loaded,
  enabled,
  trusted,
  children,
}: {
  title: string;
  detail: string;
  locale: Locale;
  selected: boolean;
  loaded: boolean;
  enabled: boolean;
  trusted?: boolean;
  children?: ReactNode;
}) {
  return (
    <li className="settings-plugin-row">
      <div className="min-w-0 space-y-2">
        <strong>{title}</strong>
        <div className="mt-1 text-xs text-[var(--text-muted)]">{detail}</div>
        {children}
      </div>
      <div className="flex flex-wrap justify-end gap-1">
        <StatusPill
          label={t(locale, 'settings.engineCapabilities.configured')}
          tone="neutral"
        />
        {selected ? (
          <StatusPill
            label={t(locale, 'settings.engineCapabilities.selected')}
            tone="info"
          />
        ) : null}
        {loaded ? (
          <StatusPill
            label={t(locale, 'settings.engineCapabilities.loaded')}
            tone="success"
          />
        ) : null}
        {!enabled ? (
          <StatusPill
            label={t(locale, 'settings.engineCapabilities.disabled')}
            tone="warning"
          />
        ) : null}
        {trusted === false ? (
          <StatusPill
            label={t(locale, 'settings.engineCapabilities.untrusted')}
            tone="warning"
          />
        ) : null}
      </div>
    </li>
  );
}

function AvailabilityFlag({
  locale,
  labelKey,
  available,
}: {
  locale: Locale;
  labelKey: 'tools' | 'hooks' | 'subagents' | 'mcp' | 'extensions' | 'scheduler';
  available: boolean;
}) {
  return (
    <StatusPill
      label={`${t(locale, `settings.engineCapabilities.${labelKey}`)} · ${
        available
          ? t(locale, 'settings.engineCapabilities.available')
          : t(locale, 'settings.engineCapabilities.unavailable')
      }`}
      tone={available ? 'success' : 'warning'}
    />
  );
}

export function StatusPill({
  label,
  tone,
  dataStatus,
}: {
  label: string;
  tone: 'neutral' | 'info' | 'success' | 'warning';
  dataStatus?: string;
}) {
  const toneClass = {
    neutral: 'border-[var(--border)] text-[var(--text-muted)]',
    info: 'border-[var(--accent)] text-[var(--accent)]',
    success: 'border-[var(--success)] text-[var(--success)]',
    warning: 'border-[var(--warning)] text-[var(--warning)]',
  }[tone];
  return (
    <span
      className={`rounded-full border px-2 py-0.5 text-[11px] ${toneClass}`}
      data-status={dataStatus}
    >
      {label}
    </span>
  );
}

export function DataCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-[var(--border)] p-2">
      <div className="text-[var(--text-muted)]">{label}</div>
      <div className="mt-1 break-all font-mono">{value || '—'}</div>
    </div>
  );
}

export function HashLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-[var(--border)] p-3 text-xs">
      <strong>{label}</strong>
      <code className="mt-1 block break-all text-[var(--text-muted)]">{value}</code>
    </div>
  );
}

export function Notice({ title, description }: { title: string; description: string }) {
  return (
    <div className="settings-section-card mb-3 border-[var(--warning)]">
      <strong>{title}</strong>
      <p className="mt-1 text-xs text-[var(--text-muted)]">{description}</p>
    </div>
  );
}

export function Loading({ locale }: { locale: Locale }) {
  return (
    <div style={{ padding: SPACING.md }} className="text-sm text-[var(--text-muted)]">
      <Loader size={12} className="mr-2 inline animate-spin" />
      {t(locale, 'common.loading')}
    </div>
  );
}

export function SectionError({
  locale,
  message,
  onRetry,
}: {
  locale: Locale;
  message: string;
  onRetry: () => void;
}) {
  return (
    <div role="alert" style={{ padding: SPACING.md }} className="text-sm text-[var(--danger)]">
      <div>{message}</div>
      <button type="button" className="btn mt-2 text-xs" onClick={onRetry}>
        {t(locale, 'common.retry')}
      </button>
    </div>
  );
}

export function Unavailable({ locale }: { locale: Locale }) {
  return (
    <div className="settings-plugin-empty" style={{ padding: SPACING.md }}>
      {t(locale, 'settings.engineCapabilities.methodUnavailable')}
    </div>
  );
}

export interface SnapshotEvidenceCardsProps {
  locale: Locale;
  snapshotLoaded: boolean;
  toolPlan: { canonical_hash?: string; tools?: Array<{ name: string; source: string; schema_digest: string }> } | null;
  toolPlanHash: string | undefined;
  promptPlan: { effective_prompt_hash?: string } | null;
  promptLayers: Array<{ layer_id: string; kind: string; source_owner: string; digest: string; char_estimate: number }>;
}

/** Run snapshot evidence: tool plan + assembled prompt layers. */
export function SnapshotEvidenceCards({
  locale,
  snapshotLoaded,
  toolPlan,
  toolPlanHash,
  promptPlan,
  promptLayers,
}: SnapshotEvidenceCardsProps) {
  return (
    <div className="grid gap-3 lg:grid-cols-2">
      <EvidenceCard
        icon={<Wrench size={16} />}
        title={t(locale, 'settings.engineCapabilities.toolsTitle')}
        loaded={snapshotLoaded}
        empty={(toolPlan?.tools ?? []).length === 0}
        emptyLabel={t(locale, 'settings.engineCapabilities.toolsEmpty')}
        loadedLabel={t(locale, 'settings.engineCapabilities.loaded')}
        missingLabel={t(locale, 'settings.engineCapabilities.notLoaded')}
      >
        {toolPlanHash ? (
          <HashLine
            label={t(locale, 'settings.engineCapabilities.canonicalHash')}
            value={toolPlanHash}
          />
        ) : null}
        {(toolPlan?.tools ?? []).map((tool) => (
          <div className="rounded border border-[var(--border)] p-3 text-xs" key={tool.name}>
            <strong>{tool.name}</strong>
            <div className="mt-1 text-[var(--text-muted)]">{tool.source}</div>
            <code className="mt-1 block break-all text-[var(--text-muted)]">
              {tool.schema_digest}
            </code>
          </div>
        ))}
      </EvidenceCard>

      <EvidenceCard
        icon={<FileText size={16} />}
        title={t(locale, 'settings.engineCapabilities.promptTitle')}
        loaded={snapshotLoaded}
        empty={promptLayers.length === 0}
        emptyLabel={t(locale, 'settings.engineCapabilities.promptEmpty')}
        loadedLabel={t(locale, 'settings.engineCapabilities.loaded')}
        missingLabel={t(locale, 'settings.engineCapabilities.notLoaded')}
      >
        {promptPlan?.effective_prompt_hash ? (
          <HashLine
            label={t(locale, 'settings.engineCapabilities.effectivePromptHash')}
            value={promptPlan.effective_prompt_hash}
          />
        ) : null}
        {promptLayers.map((layer) => (
          <div
            className="rounded border border-[var(--border)] p-3 text-xs"
            key={layer.layer_id}
          >
            <strong>{layer.layer_id}</strong>
            <div className="mt-1 text-[var(--text-muted)]">
              {layer.kind} · {layer.source_owner}
            </div>
            <div className="mt-1 text-[var(--text-muted)]">
              {t(locale, 'settings.engineCapabilities.characterEstimate', {
                count: layer.char_estimate,
              })}
            </div>
            <code className="mt-1 block break-all text-[var(--text-muted)]">
              {layer.digest}
            </code>
          </div>
        ))}
      </EvidenceCard>
    </div>
  );
}

export interface RateLimitEditorProps {
  locale: Locale;
  snapshot: EngineRateLimitSnapshot;
  editEnabled: boolean;
  onEditEnabledChange: (value: boolean) => void;
  editRpm: number;
  onEditRpmChange: (value: number) => void;
  rpmValid: boolean;
  intervalSeconds: string;
  saving: boolean;
  canUpdate: boolean;
  onSave: () => void;
}

export function RateLimitEditor({
  locale,
  snapshot,
  editEnabled,
  onEditEnabledChange,
  editRpm,
  onEditRpmChange,
  rpmValid,
  intervalSeconds,
  saving,
  canUpdate,
  onSave,
}: RateLimitEditorProps) {
  return (
    <div style={{ padding: SPACING.md }}>
      <label className="mb-3 flex cursor-pointer items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={editEnabled}
          onChange={(event) => onEditEnabledChange(event.target.checked)}
        />
        {editEnabled ? t(locale, 'rateLimit.enabled') : t(locale, 'rateLimit.disabled')}
      </label>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <label className="text-sm" htmlFor="engine-rate-limit-rpm">
          {t(locale, 'rateLimit.rpm')}
        </label>
        <input
          id="engine-rate-limit-rpm"
          className="input w-28"
          type="number"
          min={1}
          max={600}
          step={1}
          value={editRpm}
          disabled={!editEnabled}
          onChange={(event) => onEditRpmChange(Number(event.target.value))}
        />
        <span className={rpmValid ? 'text-xs text-[var(--text-muted)]' : 'text-xs text-[var(--danger)]'}>
          {t(locale, 'rateLimit.rpmRange')}
        </span>
      </div>
      {editEnabled && rpmValid ? (
        <p className="mb-3 text-xs text-[var(--text-muted)]">
          {t(locale, 'rateLimit.intervalHint', { seconds: intervalSeconds })}
        </p>
      ) : null}
      <div className="mb-3 flex flex-wrap gap-4 text-xs text-[var(--text-muted)]">
        <span>
          {t(locale, 'rateLimit.queued')}: <strong>{snapshot.queued_requests}</strong>
        </span>
        <span>
          {t(locale, 'rateLimit.cooling')}: <strong>{snapshot.cooling_routes}</strong>
        </span>
      </div>
      <button
        type="button"
        className="btn btn-primary text-xs"
        disabled={saving || (!rpmValid && editEnabled) || !canUpdate}
        onClick={onSave}
      >
        {saving ? <Loader size={12} className="animate-spin" /> : null}
        {t(locale, 'rateLimit.save')}
      </button>
    </div>
  );
}

export function ExtensionsList({
  locale,
  snapshot,
}: {
  locale: Locale;
  snapshot: ExtensionAdminSnapshot;
}) {
  return (
    <InventoryList>
      {snapshot.extensions.map((extension, index) => {
        const row = extension as Record<string, unknown>;
        return (
          <li
            key={String(row.id ?? row.name ?? index)}
            className="settings-plugin-row"
          >
            <div>
              <strong>{String(row.name ?? row.id ?? index)}</strong>
              <div className="mt-1 text-xs text-[var(--text-muted)]">
                {t(
                  locale,
                  'settings.engineCapabilities.discoveredNotExecutable',
                )}
              </div>
            </div>
            <StatusPill
              label={t(
                locale,
                'settings.engineCapabilities.discoveredNotExecutable',
              )}
              tone="warning"
              dataStatus={String(
                row.execution_status ?? EXTENSION_DISCOVERY_STATUS,
              )}
            />
          </li>
        );
      })}
    </InventoryList>
  );
}
