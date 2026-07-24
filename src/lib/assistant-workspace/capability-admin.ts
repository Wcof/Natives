/**
 * Phase 6 capability admin façade — all management RPC goes through AssistantGateway.
 * UI panels must not call nativesAPI skill/mcp/scheduler stores directly for daemon authority.
 */
import type { AssistantGateway } from '@/lib/assistant-gateway';

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
