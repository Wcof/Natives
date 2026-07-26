/**
 * Phase 6 capability admin façade — all management RPC goes through AssistantGateway.
 * UI panels must not call nativesAPI skill/mcp/scheduler stores directly for daemon authority.
 * ADR-0016: every `capability.*` / `conversation.*Capabilities` RPC lives here —
 * capability components never talk to the gateway directly.
 */
import type { AssistantGateway } from '@/lib/assistant-gateway';
import type { CapabilitySelection } from '@/lib/assistant-protocol';
import type {
  CapabilityExpert,
  CapabilityExpertTeam,
  CapabilityMcpServer,
  CapabilityMcpServerInput,
  CapabilitySkill,
  CapabilitySkillDetail,
  McpHubEntry,
  McpHubSearchResult,
  McpJsonImportResult,
  SkillRescanResult,
} from '@/components/capabilities/shared/capability-types';

export interface McpAdminSnapshot {
  servers: unknown[];
  tools: unknown[];
  namespaced: unknown[];
}

export interface SchedulerAdminSnapshot {
  jobs: unknown[];
}

export interface ExtensionAdminSnapshot {
  extensions: unknown[];
}

export interface SkillAdminSnapshot {
  skills: unknown[];
}

export interface MemoryAdminSnapshot {
  results: unknown[];
}

export interface EngineRateLimitSettings {
  enabled: boolean;
  requests_per_minute: number;
}

export interface EngineRateLimitSnapshot {
  settings: EngineRateLimitSettings;
  effective_interval_ms: number;
  queued_requests: number;
  cooling_routes: number;
}

export async function listMcp(gateway: AssistantGateway): Promise<McpAdminSnapshot> {
  const raw = (await gateway.request('mcp.list', {})) as Record<string, unknown>;
  return {
    servers: (raw?.servers as unknown[]) ?? [],
    tools: (raw?.tools as unknown[]) ?? [],
    namespaced: (raw?.namespaced as unknown[]) ?? [],
  };
}

export async function listScheduler(gateway: AssistantGateway): Promise<SchedulerAdminSnapshot> {
  const raw = await gateway.request('scheduler.list', {});
  if (Array.isArray(raw)) return { jobs: raw };
  const obj = (raw ?? {}) as Record<string, unknown>;
  return { jobs: (obj.jobs as unknown[]) ?? (obj.items as unknown[]) ?? [] };
}

export async function listExtensions(gateway: AssistantGateway): Promise<ExtensionAdminSnapshot> {
  const raw = await gateway.request('extension.list', {});
  if (Array.isArray(raw)) return { extensions: raw };
  const obj = (raw ?? {}) as Record<string, unknown>;
  return { extensions: (obj.extensions as unknown[]) ?? (obj.items as unknown[]) ?? [] };
}

export async function listSkills(gateway: AssistantGateway): Promise<SkillAdminSnapshot> {
  const raw = await gateway.request('skill.list', {});
  if (Array.isArray(raw)) return { skills: raw };
  const obj = (raw ?? {}) as Record<string, unknown>;
  return { skills: (obj.skills as unknown[]) ?? (obj.items as unknown[]) ?? [] };
}

export async function searchMemory(
  gateway: AssistantGateway,
  query: string,
): Promise<MemoryAdminSnapshot> {
  const raw = await gateway.request('memory.search', { query });
  if (Array.isArray(raw)) return { results: raw };
  const obj = (raw ?? {}) as Record<string, unknown>;
  return { results: (obj.results as unknown[]) ?? (obj.items as unknown[]) ?? [] };
}

export async function getRateLimit(gateway: AssistantGateway): Promise<EngineRateLimitSnapshot> {
  return (await gateway.request('engine.rateLimit.get', {})) as EngineRateLimitSnapshot;
}

export async function updateRateLimit(
  gateway: AssistantGateway,
  settings: EngineRateLimitSettings,
): Promise<EngineRateLimitSnapshot> {
  return (await gateway.request('engine.rateLimit.update', settings)) as EngineRateLimitSnapshot;
}

// ── Capability Hub (ADR-0016) — Skills ──

export interface CapabilitySkillListFilter {
  scope?: string;
  category?: string;
  query?: string;
  enabledOnly?: boolean;
}

export async function listCapabilitySkills(
  gateway: AssistantGateway,
  filter: CapabilitySkillListFilter = {},
): Promise<CapabilitySkill[]> {
  const raw = await gateway.request<{ skills?: CapabilitySkill[] }>('capability.skill.list', filter);
  return raw?.skills ?? [];
}

export async function getCapabilitySkill(
  gateway: AssistantGateway,
  id: string,
): Promise<CapabilitySkillDetail> {
  const raw = await gateway.request<{ skill: CapabilitySkillDetail }>('capability.skill.get', { id });
  return raw.skill;
}

export async function updateCapabilitySkill(
  gateway: AssistantGateway,
  id: string,
  patch: {
    category?: string | null;
    tags?: string[];
    enabled?: boolean;
    trusted?: boolean;
    engineTargets?: string[];
  },
): Promise<void> {
  await gateway.request('capability.skill.update', { id, ...patch });
}

export async function importCapabilitySkill(
  gateway: AssistantGateway,
  input: {
    source: 'zip' | 'dir';
    path: string;
    name?: string;
    category?: string;
    tags?: string[];
    linkClaudeDir?: boolean;
  },
): Promise<void> {
  await gateway.request('capability.skill.import', input);
}

export async function deleteCapabilitySkill(
  gateway: AssistantGateway,
  id: string,
  mode?: 'unregister' | 'remove_dir',
): Promise<void> {
  await gateway.request('capability.skill.delete', { id, ...(mode ? { mode } : {}) });
}

export async function rescanCapabilitySkills(
  gateway: AssistantGateway,
  projectRoot?: string,
): Promise<SkillRescanResult> {
  return await gateway.request<SkillRescanResult>(
    'capability.skill.rescan',
    projectRoot ? { projectRoot } : {},
  );
}

// ── Capability Hub — MCP connectors ──

export async function listCapabilityMcpServers(
  gateway: AssistantGateway,
  enabledOnly = false,
): Promise<CapabilityMcpServer[]> {
  const raw = await gateway.request<{ servers?: CapabilityMcpServer[] }>('capability.mcp.list', {
    enabledOnly,
  });
  return raw?.servers ?? [];
}

export async function getCapabilityMcpServer(
  gateway: AssistantGateway,
  id: string,
): Promise<CapabilityMcpServer> {
  const raw = await gateway.request<{ server: CapabilityMcpServer }>('capability.mcp.get', { id });
  return raw.server;
}

export async function createCapabilityMcpServer(
  gateway: AssistantGateway,
  input: CapabilityMcpServerInput,
): Promise<void> {
  await gateway.request('capability.mcp.create', input);
}

export async function updateCapabilityMcpServer(
  gateway: AssistantGateway,
  id: string,
  patch: Partial<CapabilityMcpServerInput>,
): Promise<void> {
  await gateway.request('capability.mcp.update', { id, ...patch });
}

export async function deleteCapabilityMcpServer(
  gateway: AssistantGateway,
  id: string,
): Promise<void> {
  await gateway.request('capability.mcp.delete', { id });
}

export async function importCapabilityMcpJson(
  gateway: AssistantGateway,
  json: string,
): Promise<McpJsonImportResult> {
  const raw = await gateway.request<Partial<McpJsonImportResult>>('capability.mcp.importJson', {
    json,
  });
  return {
    imported: raw?.imported ?? [],
    skipped: raw?.skipped ?? 0,
    errors: raw?.errors ?? [],
  };
}

// ── Capability Hub — MCP online hub (gated by capability.mcp.hub.search) ──

export async function searchMcpHub(
  gateway: AssistantGateway,
  params: { query?: string; cursor?: string; limit?: number } = {},
): Promise<McpHubSearchResult> {
  const raw = await gateway.request<Partial<McpHubSearchResult>>('capability.mcp.hub.search', params);
  return {
    servers: raw?.servers ?? [],
    nextCursor: raw?.nextCursor ?? null,
    stale: raw?.stale ?? false,
  };
}

export async function getMcpHubEntry(
  gateway: AssistantGateway,
  registryName: string,
): Promise<McpHubEntry> {
  const raw = await gateway.request<{ server: McpHubEntry }>('capability.mcp.hub.get', {
    registryName,
  });
  return raw.server;
}

export async function installMcpHubEntry(
  gateway: AssistantGateway,
  input: {
    registryName: string;
    packageIndex?: number;
    remoteIndex?: number;
    envOverrides?: Record<string, string>;
  },
): Promise<void> {
  await gateway.request('capability.mcp.hub.install', input);
}

// ── Capability Hub — Experts & teams ──

export async function listCapabilityExperts(
  gateway: AssistantGateway,
  params: { query?: string; enabledOnly?: boolean } = {},
): Promise<CapabilityExpert[]> {
  const raw = await gateway.request<{ experts?: CapabilityExpert[] }>('capability.expert.list', params);
  return raw?.experts ?? [];
}

export async function createCapabilityExpert(
  gateway: AssistantGateway,
  input: Partial<CapabilityExpert>,
): Promise<void> {
  await gateway.request('capability.expert.create', input);
}

export async function updateCapabilityExpert(
  gateway: AssistantGateway,
  id: string,
  patch: Partial<CapabilityExpert>,
): Promise<void> {
  await gateway.request('capability.expert.update', { id, ...patch });
}

export async function deleteCapabilityExpert(
  gateway: AssistantGateway,
  id: string,
  force = false,
): Promise<void> {
  await gateway.request('capability.expert.delete', { id, ...(force ? { force: true } : {}) });
}

export async function importCapabilityExpertMd(
  gateway: AssistantGateway,
  input: { content?: string; path?: string },
): Promise<void> {
  await gateway.request('capability.expert.importMd', input);
}

export async function exportCapabilityExpertMd(
  gateway: AssistantGateway,
  id: string,
): Promise<string> {
  const raw = await gateway.request<{ content?: string }>('capability.expert.exportMd', { id });
  return raw?.content ?? '';
}

export async function listCapabilityTeams(
  gateway: AssistantGateway,
): Promise<CapabilityExpertTeam[]> {
  const raw = await gateway.request<{ teams?: CapabilityExpertTeam[] }>('capability.team.list', {});
  return raw?.teams ?? [];
}

export async function createCapabilityTeam(
  gateway: AssistantGateway,
  input: Partial<CapabilityExpertTeam>,
): Promise<void> {
  await gateway.request('capability.team.create', input);
}

export async function updateCapabilityTeam(
  gateway: AssistantGateway,
  id: string,
  patch: Partial<CapabilityExpertTeam>,
): Promise<void> {
  await gateway.request('capability.team.update', { id, ...patch });
}

export async function deleteCapabilityTeam(gateway: AssistantGateway, id: string): Promise<void> {
  await gateway.request('capability.team.delete', { id });
}

// ── Conversation capability selection (gated by conversation.updateCapabilities) ──

export async function updateConversationCapabilities(
  gateway: AssistantGateway,
  conversationId: string,
  selection: CapabilitySelection | null,
): Promise<void> {
  await gateway.request('conversation.updateCapabilities', {
    conversation_id: conversationId,
    selection,
  });
}

export async function getConversationCapabilities(
  gateway: AssistantGateway,
  conversationId: string,
): Promise<CapabilitySelection | null> {
  const raw = await gateway.request<{ selection?: CapabilitySelection | null }>(
    'conversation.getCapabilities',
    { conversation_id: conversationId },
  );
  return raw?.selection ?? null;
}

/** Aggregate dashboard for settings / management surfaces. */
export async function loadCapabilityAdminDashboard(gateway: AssistantGateway): Promise<{
  mcp: McpAdminSnapshot;
  scheduler: SchedulerAdminSnapshot;
  extensions: ExtensionAdminSnapshot;
  skills: SkillAdminSnapshot;
  rateLimit: EngineRateLimitSnapshot | null;
}> {
  const [mcp, scheduler, extensions, skills, rateLimit] = await Promise.all([
    listMcp(gateway),
    listScheduler(gateway),
    listExtensions(gateway),
    listSkills(gateway),
    getRateLimit(gateway).catch(() => null),
  ]);
  return { mcp, scheduler, extensions, skills, rateLimit };
}
