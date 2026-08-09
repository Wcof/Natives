'use client';

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from 'react';
import {
  Activity,
  Clock,
  FileText,
  Loader,
  Puzzle,
  RefreshCw,
  Server,
  Sparkles,
  Users,
  Wrench,
  Zap,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { createDefaultGateway } from '@/lib/assistant-gateway';
import type { DaemonCapabilities } from '@/lib/assistant-protocol';
import {
  getRateLimit,
  listCapabilityExperts,
  listCapabilityMcpServers,
  listCapabilitySkills,
  listCapabilityTeams,
  listExtensions,
  updateRateLimit,
  type EngineRateLimitSnapshot,
  type ExtensionAdminSnapshot,
} from '@/lib/assistant-workspace/capability-admin';
import { hasMethod } from '@/lib/assistant-workspace/capability-gate';
import type {
  CapabilityExpert,
  CapabilityExpertTeam,
  CapabilityMcpServer,
  CapabilitySkill,
} from '@/types/capability';
import { classifyError } from '@/lib/error-classifier';
import { SPACING } from '@/lib/design-tokens';
import { useToast } from '@/components/ui/Toast';
import type { CanvasRunSnapshot } from './nativeExecutionCanvasModel';

const EXTENSION_DISCOVERY_STATUS = 'discovered_not_executable' as const;

export type EngineCapabilitySnapshot = {
  selectionActive?: boolean;
  agentProfileId?: string | null;
  teamId?: string | null;
  teamMembers?: string[] | null;
  skillIds?: string[];
  mcpServers?: string[];
};

export type EngineCapabilityRun = {
  id: string;
  status: string;
  provider_id: string;
  model_id: string;
  permission_profile: string;
  runtime_id?: string | null;
  agent_profile_id?: string | null;
  capability_snapshot?: EngineCapabilitySnapshot | null;
};

export interface EngineCapabilitiesPanelProps {
  locale: Locale;
  selectedRun: EngineCapabilityRun | null;
  runSnapshot: CanvasRunSnapshot | null;
}

type Loadable<T> =
  | { phase: 'idle' }
  | { phase: 'loading' }
  | { phase: 'success'; data: T }
  | { phase: 'error'; message: string }
  | { phase: 'unavailable' };

type LoadSectionInput<T> = {
  advertised: boolean;
  locale: Locale;
  loader: () => Promise<T>;
  setState: Dispatch<SetStateAction<Loadable<T>>>;
  isCurrent: () => boolean;
};

async function loadSection<T>({
  advertised,
  locale,
  loader,
  setState,
  isCurrent,
}: LoadSectionInput<T>): Promise<void> {
  if (!isCurrent()) return;
  if (!advertised) {
    setState({ phase: 'unavailable' });
    return;
  }
  setState({ phase: 'loading' });
  try {
    const data = await loader();
    if (isCurrent()) setState({ phase: 'success', data });
  } catch (error) {
    if (isCurrent()) {
      setState({
        phase: 'error',
        message: classifyError(error, { locale }).userMessage,
      });
    }
  }
}

const initialLoadable = <T,>(): Loadable<T> => ({ phase: 'idle' });

export default function EngineCapabilitiesPanel({
  locale,
  selectedRun,
  runSnapshot,
}: EngineCapabilitiesPanelProps) {
  const gateway = useMemo(() => createDefaultGateway(false), []);
  const requestGeneration = useRef(0);
  const { toast } = useToast();
  const [capabilities, setCapabilities] = useState<Loadable<DaemonCapabilities>>(
    initialLoadable,
  );
  const [skills, setSkills] = useState<Loadable<CapabilitySkill[]>>(initialLoadable);
  const [mcpServers, setMcpServers] = useState<Loadable<CapabilityMcpServer[]>>(
    initialLoadable,
  );
  const [experts, setExperts] = useState<Loadable<CapabilityExpert[]>>(initialLoadable);
  const [teams, setTeams] = useState<Loadable<CapabilityExpertTeam[]>>(initialLoadable);
  const [extensions, setExtensions] = useState<Loadable<ExtensionAdminSnapshot>>(
    initialLoadable,
  );
  const [rateLimit, setRateLimit] = useState<Loadable<EngineRateLimitSnapshot>>(
    initialLoadable,
  );
  const [editEnabled, setEditEnabled] = useState(true);
  const [editRpm, setEditRpm] = useState(10);
  const [saving, setSaving] = useState(false);

  const loadSkills = useCallback(
    (caps: DaemonCapabilities, generation = requestGeneration.current) =>
      loadSection({
        advertised: hasMethod(caps, 'capability.skill.list'),
        locale,
        loader: () => listCapabilitySkills(gateway),
        setState: setSkills,
        isCurrent: () => generation === requestGeneration.current,
      }),
    [gateway, locale],
  );

  const loadMcpServers = useCallback(
    (caps: DaemonCapabilities, generation = requestGeneration.current) =>
      loadSection({
        advertised: hasMethod(caps, 'capability.mcp.list'),
        locale,
        loader: () => listCapabilityMcpServers(gateway),
        setState: setMcpServers,
        isCurrent: () => generation === requestGeneration.current,
      }),
    [gateway, locale],
  );

  const loadExperts = useCallback(
    (caps: DaemonCapabilities, generation = requestGeneration.current) =>
      loadSection({
        advertised: hasMethod(caps, 'capability.expert.list'),
        locale,
        loader: () => listCapabilityExperts(gateway),
        setState: setExperts,
        isCurrent: () => generation === requestGeneration.current,
      }),
    [gateway, locale],
  );

  const loadTeams = useCallback(
    (caps: DaemonCapabilities, generation = requestGeneration.current) =>
      loadSection({
        advertised: hasMethod(caps, 'capability.team.list'),
        locale,
        loader: () => listCapabilityTeams(gateway),
        setState: setTeams,
        isCurrent: () => generation === requestGeneration.current,
      }),
    [gateway, locale],
  );

  const loadExtensions = useCallback(
    (caps: DaemonCapabilities, generation = requestGeneration.current) =>
      loadSection({
        advertised: hasMethod(caps, 'extension.list'),
        locale,
        loader: () => listExtensions(gateway),
        setState: setExtensions,
        isCurrent: () => generation === requestGeneration.current,
      }),
    [gateway, locale],
  );

  const loadRateLimit = useCallback(
    (caps: DaemonCapabilities, generation = requestGeneration.current) =>
      loadSection({
        advertised: hasMethod(caps, 'engine.rateLimit.get'),
        locale,
        loader: async () => {
          const snapshot = await getRateLimit(gateway);
          if (generation === requestGeneration.current) {
            setEditEnabled(snapshot.settings.enabled);
            setEditRpm(snapshot.settings.requests_per_minute);
          }
          return snapshot;
        },
        setState: setRateLimit,
        isCurrent: () => generation === requestGeneration.current,
      }),
    [gateway, locale],
  );

  const load = useCallback(async () => {
    const generation = ++requestGeneration.current;
    const isCurrent = () => generation === requestGeneration.current;
    setCapabilities({ phase: 'loading' });
    try {
      await gateway.connect();
      const advertised = await gateway.getCapabilities?.();
      if (!advertised) throw new Error('DAEMON_CAPABILITIES_UNAVAILABLE');
      if (!isCurrent()) return;
      setCapabilities({ phase: 'success', data: advertised });
      await Promise.all([
        loadSkills(advertised, generation),
        loadMcpServers(advertised, generation),
        loadExperts(advertised, generation),
        loadTeams(advertised, generation),
        loadExtensions(advertised, generation),
        loadRateLimit(advertised, generation),
      ]);
    } catch (error) {
      if (!isCurrent()) return;
      setCapabilities({
        phase: 'error',
        message: classifyError(error, { locale }).userMessage,
      });
    }
  }, [
    gateway,
    loadExperts,
    loadExtensions,
    loadMcpServers,
    loadRateLimit,
    loadSkills,
    loadTeams,
    locale,
  ]);

  useEffect(() => {
    void load();
    return () => {
      requestGeneration.current += 1;
    };
  }, [load]);

  const caps = capabilities.phase === 'success' ? capabilities.data : null;
  const selection = selectedRun?.capability_snapshot ?? null;
  const selectedSkillIds = selection?.skillIds ?? [];
  const selectedMcpIds = selection?.mcpServers ?? [];
  const selectedExpertId = selection?.agentProfileId ?? null;
  const selectedTeamId = selection?.teamId ?? null;
  const snapshotLoaded = Boolean(runSnapshot?.resolved);
  const promptPlan = runSnapshot?.snapshot?.prompt_plan;
  const toolPlan = runSnapshot?.snapshot?.tool_plan;
  const promptLayers = runSnapshot?.snapshot?.prompt_plan
    ? runSnapshot.snapshot.prompt_plan.layers ?? []
    : [];
  const toolPlanHash = runSnapshot?.snapshot?.tool_plan
    ? runSnapshot.snapshot.tool_plan.canonical_hash
    : undefined;
  const rpmValid = Number.isInteger(editRpm) && editRpm >= 1 && editRpm <= 600;
  const intervalSeconds = rpmValid ? (60000 / editRpm / 1000).toFixed(1) : '—';

  const handleSaveRateLimit = useCallback(async () => {
    if (!Number.isInteger(editRpm) || editRpm < 1 || editRpm > 600) {
      toast(t(locale, 'rateLimit.invalidRpm'), 'error');
      return;
    }
    setSaving(true);
    try {
      const snapshot = await updateRateLimit(gateway, {
        enabled: editEnabled,
        requests_per_minute: editRpm,
      });
      setRateLimit({ phase: 'success', data: snapshot });
      setEditEnabled(snapshot.settings.enabled);
      setEditRpm(snapshot.settings.requests_per_minute);
      toast(t(locale, 'rateLimit.saveSuccess'), 'success');
    } catch (error) {
      toast(classifyError(error, { locale }).userMessage, 'error');
    } finally {
      setSaving(false);
    }
  }, [editEnabled, editRpm, gateway, locale, toast]);

  return (
    <div data-testid="engine-capabilities-panel">
      <div className="mb-3 flex justify-end">
        <button
          type="button"
          className="btn inline-flex items-center gap-2 text-xs"
          onClick={() => void load()}
          disabled={capabilities.phase === 'loading'}
        >
          {capabilities.phase === 'loading' ? (
            <Loader size={12} className="animate-spin" />
          ) : (
            <RefreshCw size={12} />
          )}
          {t(locale, 'common.refresh')}
        </button>
      </div>

      <AvailabilityCard
        locale={locale}
        state={capabilities}
        onRetry={() => void load()}
      />

      {selectedRun ? (
        <RunEvidenceCard
          locale={locale}
          run={selectedRun}
          snapshotLoaded={snapshotLoaded}
          selectedSkillIds={selectedSkillIds}
          selectedMcpIds={selectedMcpIds}
          selectedExpertId={selectedExpertId}
          selectedTeamId={selectedTeamId}
        />
      ) : (
        <Notice
          title={t(locale, 'settings.engineCapabilities.noRunTitle')}
          description={t(locale, 'settings.engineCapabilities.noRunDesc')}
        />
      )}

      {selectedRun ? (
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
      ) : null}

      <div className="settings-section-card mb-3">
        <h4>{t(locale, 'settings.engineCapabilities.inventoryTitle')}</h4>
        <p className="mt-1 text-xs text-[var(--text-muted)]">
          {t(locale, 'settings.engineCapabilities.inventoryDesc')}
        </p>
      </div>

      <LoadableSection
        icon={<Sparkles size={16} />}
        title={t(locale, 'settings.engineCapabilities.skillsTitle')}
        state={skills}
        count={(items) => items.length}
        empty={t(locale, 'settings.engineCapabilities.skillsEmpty')}
        locale={locale}
        onRetry={() => {
          if (caps) void loadSkills(caps);
        }}
        render={(items) => (
          <InventoryList>
            {items.map((skill) => {
              const selected = selectedSkillIds.includes(skill.id);
              return (
                <InventoryRow
                  key={skill.id}
                  title={skill.name}
                  detail={skill.id}
                  locale={locale}
                  selected={selected}
                  loaded={selected && snapshotLoaded}
                  enabled={skill.enabled}
                  trusted={skill.trusted}
                />
              );
            })}
          </InventoryList>
        )}
      />

      <LoadableSection
        icon={<Server size={16} />}
        title={t(locale, 'settings.engineCapabilities.mcpTitle')}
        state={mcpServers}
        count={(items) => items.length}
        empty={t(locale, 'settings.engineCapabilities.mcpEmpty')}
        locale={locale}
        onRetry={() => {
          if (caps) void loadMcpServers(caps);
        }}
        render={(items) => (
          <InventoryList>
            {items.map((server) => {
              const selected = selectedMcpIds.includes(server.id);
              return (
                <InventoryRow
                  key={server.id}
                  title={server.name}
                  detail={`${server.id} · ${server.transport}`}
                  locale={locale}
                  selected={selected}
                  loaded={selected && snapshotLoaded}
                  enabled={server.enabled}
                  trusted={server.trusted}
                />
              );
            })}
          </InventoryList>
        )}
      />

      <div className="grid gap-3 lg:grid-cols-2">
        <LoadableSection
          icon={<Users size={16} />}
          title={t(locale, 'settings.engineCapabilities.expertsTitle')}
          state={experts}
          count={(items) => items.length}
          empty={t(locale, 'settings.engineCapabilities.expertsEmpty')}
          locale={locale}
          onRetry={() => {
            if (caps) void loadExperts(caps);
          }}
          render={(items) => (
            <InventoryList>
              {items.map((expert) => {
                const selected = expert.id === selectedExpertId;
                return (
                  <InventoryRow
                    key={expert.id}
                    title={expert.name}
                    detail={[expert.id, expert.modelId, expert.source].filter(Boolean).join(' · ')}
                    locale={locale}
                    selected={selected}
                    loaded={selected && snapshotLoaded}
                    enabled={expert.enabled}
                  >
                    {expert.description ? <p className="m-0 text-xs text-[var(--text-muted)]">{expert.description}</p> : null}
                    <details className="text-xs">
                      <summary className="cursor-pointer text-[var(--text-secondary)]">{t(locale, 'settings.engineCapabilities.systemPrompt')}</summary>
                      <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap rounded bg-[var(--background)] p-2 font-mono text-[11px] text-[var(--text-secondary)]">{expert.systemPrompt || t(locale, 'settings.engineCapabilities.promptEmpty')}</pre>
                    </details>
                  </InventoryRow>
                );
              })}
            </InventoryList>
          )}
        />

        <LoadableSection
          icon={<Users size={16} />}
          title={t(locale, 'settings.engineCapabilities.teamsTitle')}
          state={teams}
          count={(items) => items.length}
          empty={t(locale, 'settings.engineCapabilities.teamsEmpty')}
          locale={locale}
          onRetry={() => {
            if (caps) void loadTeams(caps);
          }}
          render={(items) => (
            <InventoryList>
              {items.map((team) => {
                const selected = team.id === selectedTeamId;
                return (
                  <InventoryRow
                    key={team.id}
                    title={team.name}
                    detail={t(locale, 'settings.engineCapabilities.teamDetail', {
                      id: team.id,
                      count: team.members.length,
                      coordinator: team.coordinatorExpertId ?? '—',
                    })}
                    locale={locale}
                    selected={selected}
                    loaded={selected && snapshotLoaded}
                    enabled={team.enabled}
                  >
                    {team.description ? <p className="m-0 text-xs text-[var(--text-muted)]">{team.description}</p> : null}
                    {team.members.length ? (
                      <div className="flex flex-wrap gap-1">
                        {team.members.map((member) => (
                          <code key={`${team.id}:${member.expertId}`} className="rounded bg-[var(--background)] px-1.5 py-0.5 text-[11px] text-[var(--text-secondary)]">
                            {member.expertId}
                          </code>
                        ))}
                      </div>
                    ) : null}
                  </InventoryRow>
                );
              })}
            </InventoryList>
          )}
        />
      </div>

      <LoadableSection
        icon={<Zap size={16} />}
        title={t(locale, 'rateLimit.title')}
        state={rateLimit}
        empty={t(locale, 'settings.engineCapabilities.rateLimitEmpty')}
        locale={locale}
        onRetry={() => {
          if (caps) void loadRateLimit(caps);
        }}
        render={(snapshot) => (
          <div style={{ padding: SPACING.md }}>
            <label className="mb-3 flex cursor-pointer items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={editEnabled}
                onChange={(event) => setEditEnabled(event.target.checked)}
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
                onChange={(event) => setEditRpm(Number(event.target.value))}
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
              disabled={
                saving ||
                (!rpmValid && editEnabled) ||
                !hasMethod(caps, 'engine.rateLimit.update')
              }
              onClick={() => void handleSaveRateLimit()}
            >
              {saving ? <Loader size={12} className="animate-spin" /> : null}
              {t(locale, 'rateLimit.save')}
            </button>
          </div>
        )}
      />

      <LoadableSection
        icon={<Puzzle size={16} />}
        title={t(locale, 'settings.engineCapabilities.extensionsTitle')}
        state={extensions}
        count={(snapshot) => snapshot.extensions.length}
        empty={t(locale, 'settings.engineCapabilities.extensionsEmpty')}
        locale={locale}
        onRetry={() => {
          if (caps) void loadExtensions(caps);
        }}
        description={t(locale, 'settings.engineCapabilities.extensionsDesc')}
        render={(snapshot) => (
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
        )}
      />

      <div className="settings-section-card mb-3">
        <div className="settings-section-heading settings-section-heading-with-icon">
          <span className="settings-preference-icon">
            <Clock size={16} />
          </span>
          <div>
            <h4>{t(locale, 'settings.engineCapabilities.jobsTitle')}</h4>
            <p className="text-xs text-[var(--text-muted)]">
              {t(locale, 'settings.engineCapabilities.jobsDesc')}
            </p>
          </div>
        </div>
        <div style={{ padding: SPACING.md }}>
          <a href="/jobs" className="btn text-xs">
            {t(locale, 'settings.engineCapabilities.openJobs')}
          </a>
        </div>
      </div>
    </div>
  );
}

function AvailabilityCard({
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

function RunEvidenceCard({
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

function LoadableSection<T>({
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

function EvidenceCard({
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

function InventoryList({ children }: { children: ReactNode }) {
  return (
    <ul className="settings-plugin-list m-0 list-none p-0">
      {children}
    </ul>
  );
}

function InventoryRow({
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

function StatusPill({
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

function DataCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-[var(--border)] p-2">
      <div className="text-[var(--text-muted)]">{label}</div>
      <div className="mt-1 break-all font-mono">{value || '—'}</div>
    </div>
  );
}

function HashLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-[var(--border)] p-3 text-xs">
      <strong>{label}</strong>
      <code className="mt-1 block break-all text-[var(--text-muted)]">{value}</code>
    </div>
  );
}

function Notice({ title, description }: { title: string; description: string }) {
  return (
    <div className="settings-section-card mb-3 border-[var(--warning)]">
      <strong>{title}</strong>
      <p className="mt-1 text-xs text-[var(--text-muted)]">{description}</p>
    </div>
  );
}

function Loading({ locale }: { locale: Locale }) {
  return (
    <div style={{ padding: SPACING.md }} className="text-sm text-[var(--text-muted)]">
      <Loader size={12} className="mr-2 inline animate-spin" />
      {t(locale, 'common.loading')}
    </div>
  );
}

function SectionError({
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

function Unavailable({ locale }: { locale: Locale }) {
  return (
    <div className="settings-plugin-empty" style={{ padding: SPACING.md }}>
      {t(locale, 'settings.engineCapabilities.methodUnavailable')}
    </div>
  );
}
