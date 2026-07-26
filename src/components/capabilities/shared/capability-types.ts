/**
 * Capability Hub frontend types (ADR-0016).
 * Wire shapes are the frozen daemon contracts — camelCase fields as returned
 * by `capability.*` RPC. Components must consume these via the
 * `capability-admin` façade, never raw gateway payloads.
 */

/** Skill category vocabulary; `null` / '' = uncategorized. */
export const SKILL_CATEGORIES = ['office', 'tools', 'investment', 'productivity', 'other'] as const;
export type SkillCategory = (typeof SKILL_CATEGORIES)[number];

/** Category filter values: 'all' | concrete category | 'uncategorized'. */
export type CategoryFilterValue = 'all' | SkillCategory | 'uncategorized';

export interface CapabilitySkill {
  id: string;
  name: string;
  description: string;
  scope: string;
  dirPath: string;
  category: string | null;
  tags: string[];
  enabled: boolean;
  trusted: boolean;
  source: string;
  engineTargets: string[];
  createdAt: string;
  updatedAt: string;
}

export interface CapabilitySkillDetail extends CapabilitySkill {
  bodyPreview: string;
}

export interface SkillRescanResult {
  scanned: number;
  new: number;
  changed: number;
  missing: string[];
}

export type McpTransport = 'stdio' | 'http' | 'sse';

export interface CapabilityMcpEnvEntry {
  key: string;
  isSecretRef: boolean;
}

export interface CapabilityMcpServer {
  id: string;
  name: string;
  transport: McpTransport;
  command: string | null;
  args: string[];
  env: CapabilityMcpEnvEntry[];
  url: string | null;
  headerKeys: string[];
  authMode: string | null;
  trusted: boolean;
  enabled: boolean;
  source: string;
  hubRef: string | null;
  runtimeStatus: string | null;
}

/** Create/update payload — env/headers carry raw values (or `secret:<id>` refs). */
export interface CapabilityMcpServerInput {
  id?: string;
  name: string;
  transport: McpTransport;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
  authMode?: string;
  trusted?: boolean;
  enabled?: boolean;
}

export interface McpJsonImportResult {
  imported: string[];
  skipped: number;
  errors: string[];
}

export interface McpHubEntry {
  registryName: string;
  name?: string;
  description?: string;
  version?: string;
  installed: boolean;
  [key: string]: unknown;
}

export interface McpHubSearchResult {
  servers: McpHubEntry[];
  nextCursor: string | null;
  stale: boolean;
}

export interface CapabilityExpert {
  id: string;
  name: string;
  description: string;
  systemPrompt: string;
  tools: string[];
  disallowedTools: string[];
  permissionMode: string | null;
  skills: string[];
  providerId: string | null;
  keyId: string | null;
  modelId: string | null;
  params: Record<string, unknown> | null;
  enabled: boolean;
  source: string;
}

export interface CapabilityTeamMember {
  expertId: string;
  position: number;
  roleHint: string | null;
  taskTemplate: string | null;
}

export interface CapabilityExpertTeam {
  id: string;
  name: string;
  description: string;
  strategy: string;
  failurePolicy: string;
  maxConcurrent: number;
  coordinatorExpertId: string | null;
  enabled: boolean;
  members: CapabilityTeamMember[];
}
