'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
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
import { useToast } from '@/components/ui/Toast';
import type { CanvasRunSnapshot } from '../nativeExecutionCanvasModel';
import {
  initialLoadable,
  loadSection,
  type EngineCapabilityRun,
  type Loadable,
} from './model';

export interface EngineCapabilitiesPanelProps {
  locale: Locale;
  selectedRun: EngineCapabilityRun | null;
  runSnapshot: CanvasRunSnapshot | null;
}

type SnapshotPromptPlan = NonNullable<NonNullable<CanvasRunSnapshot['snapshot']>['prompt_plan']>;
type SnapshotToolPlan = NonNullable<NonNullable<CanvasRunSnapshot['snapshot']>['tool_plan']>;

export interface EngineCapabilitiesController {
  locale: Locale;
  capabilities: Loadable<DaemonCapabilities>;
  skills: Loadable<CapabilitySkill[]>;
  mcpServers: Loadable<CapabilityMcpServer[]>;
  experts: Loadable<CapabilityExpert[]>;
  teams: Loadable<CapabilityExpertTeam[]>;
  extensions: Loadable<ExtensionAdminSnapshot>;
  rateLimit: Loadable<EngineRateLimitSnapshot>;
  editEnabled: boolean;
  setEditEnabled: (value: boolean) => void;
  editRpm: number;
  setEditRpm: (value: number) => void;
  saving: boolean;
  reload: () => Promise<void>;
  loadSkills: (caps: DaemonCapabilities) => Promise<void>;
  loadMcpServers: (caps: DaemonCapabilities) => Promise<void>;
  loadExperts: (caps: DaemonCapabilities) => Promise<void>;
  loadTeams: (caps: DaemonCapabilities) => Promise<void>;
  loadExtensions: (caps: DaemonCapabilities) => Promise<void>;
  loadRateLimit: (caps: DaemonCapabilities) => Promise<void>;
  saveRateLimit: () => Promise<void>;
  caps: DaemonCapabilities | null;
  snapshotLoaded: boolean;
  promptPlan: SnapshotPromptPlan | null;
  toolPlan: SnapshotToolPlan | null;
  promptLayers: NonNullable<SnapshotPromptPlan['layers']>;
  toolPlanHash: string | undefined;
  rpmValid: boolean;
  intervalSeconds: string;
  selectedSkillIds: string[];
  selectedMcpIds: string[];
  selectedExpertId: string | null;
  selectedTeamId: string | null;
}

export function useEngineCapabilities({
  locale,
  selectedRun,
  runSnapshot,
}: EngineCapabilitiesPanelProps): EngineCapabilitiesController {
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

  const reload = useCallback(async () => {
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
    void reload();
    return () => {
      requestGeneration.current += 1;
    };
  }, [reload]);

  const caps = capabilities.phase === 'success' ? capabilities.data : null;
  const selection = selectedRun?.capability_snapshot ?? null;
  const selectedSkillIds = selection?.skillIds ?? [];
  const selectedMcpIds = selection?.mcpServers ?? [];
  const selectedExpertId = selection?.agentProfileId ?? null;
  const selectedTeamId = selection?.teamId ?? null;
  const snapshotLoaded = Boolean(runSnapshot?.resolved);
  const promptPlan = runSnapshot?.snapshot?.prompt_plan ?? null;
  const toolPlan = runSnapshot?.snapshot?.tool_plan ?? null;
  const promptLayers = promptPlan?.layers ?? [];
  const toolPlanHash = toolPlan?.canonical_hash ?? undefined;
  const rpmValid = Number.isInteger(editRpm) && editRpm >= 1 && editRpm <= 600;
  const intervalSeconds = rpmValid ? (60000 / editRpm / 1000).toFixed(1) : '—';

  const saveRateLimit = useCallback(async () => {
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

  return {
    locale,
    capabilities,
    skills,
    mcpServers,
    experts,
    teams,
    extensions,
    rateLimit,
    editEnabled,
    setEditEnabled,
    editRpm,
    setEditRpm,
    saving,
    reload,
    loadSkills,
    loadMcpServers,
    loadExperts,
    loadTeams,
    loadExtensions,
    loadRateLimit,
    saveRateLimit,
    caps,
    snapshotLoaded,
    promptPlan,
    toolPlan,
    promptLayers,
    toolPlanHash,
    rpmValid,
    intervalSeconds,
    selectedSkillIds,
    selectedMcpIds,
    selectedExpertId,
    selectedTeamId,
  };
}
