/**
 * tauri/assistant — Agent / 助手 / 库 域 facade（ARCH-002）
 *
 * agent / skills / library / subagent / assistantV2 / project 统一入口。
 * 业务组件只允许经本 facade 访问；唯一 raw invoke 在 ./core.ts。
 */

import { cmd } from './core';
import type { NativesAPI, HarnessNotice, ProjectSummary } from './types';
import { unwrapAssistantRpc, type AssistantRpcEnvelope } from '../assistant-rpc';

  // Agent
export const agent: NativesAPI['agent'] = {
    scanProjects: () => cmd('agent_scan_projects'),
    getSessions: (projectPath: string) => cmd('agent_get_sessions', { projectPath }),
    scanSkills: () => cmd('agent_scan_skills'),
    detectStatus: (output: string, exitCode?: number) =>
      cmd('agent_detect_status', { output, exitCode }),
};

  // Skills
export const skills: NativesAPI['skills'] = {
    enable: (path: string) => cmd('skills_enable', { path }),
    disable: (path: string) => cmd('skills_disable', { path }),
    getDeactivatedPath: (path: string) => cmd('skills_get_deactivated_path', { path }),
    uninstall: (path: string) => cmd('skills_uninstall', { path }),
};

  // ── Library (fanbox clone — G4) ──
export const library: NativesAPI['library'] = {
    listFolders: () => cmd('library_list_folders'),
    createFolder: (data: { name: string; parentId?: string }) =>
      cmd('library_create_folder', { input: data }),
    updateFolder: (data: { id: string; name: string }) =>
      cmd('library_update_folder', { input: data }),
    deleteFolder: (id: string, moveItems: boolean) =>
      cmd('library_delete_folder', { id, moveItems }),
    listTags: () => cmd('library_list_tags'),
    createTag: (data: { name: string; color: string }) =>
      cmd('library_create_tag', { input: data }),
    deleteTag: (id: string) =>
      cmd('library_delete_tag', { id }),
    listItems: (filter: {
      folderId?: string; tagId?: string; keyword?: string;
      status?: string; itemType?: string; limit?: number; offset?: number;
    }) => cmd('library_list_items', { filter }),
    getItem: (id: string) =>
      cmd('library_get_item', { id }),
    createItem: (data: {
      folderId?: string; title: string; description?: string;
      content?: string; sourceUrl?: string; itemType?: string;
      status?: string; tagIds?: string[];
    }) => cmd('library_create_item', { input: data }),
    updateItem: (data: {
      id: string; folderId?: string | null; title?: string; description?: string;
      content?: string; sourceUrl?: string; status?: string; tagIds?: string[];
    }) => cmd('library_update_item', { input: data }),
    deleteItem: (id: string) =>
      cmd('library_delete_item', { id }),
    batchTag: (data: { itemIds: string[]; tagIds: string[] }) =>
      cmd('library_batch_tag', { input: data }),
    batchMove: (data: { itemIds: string[]; folderId?: string }) =>
      cmd('library_batch_move', { input: data }),
    batchDelete: (data: { itemIds: string[] }) =>
      cmd('library_batch_delete', { input: data }),
    getStats: () => cmd('library_get_stats'),
};

  // ── Subagent (G8) ──
export const subagent: NativesAPI['subagent'] = {
    list: () => cmd('subagent_list'),
    get: (id: string) => cmd('subagent_get', { id }),
    create: (data: {
      name: string; role?: string; instructions?: string; tools?: string;
      providerId?: string; providerKeyId?: string; modelId?: string; fallbackEnabled?: boolean; maxRuns?: number;
    }) => cmd('subagent_create', { input: data }),
    update: (data: {
      id: string; name: string; role?: string; instructions?: string; tools?: string;
      providerId?: string; providerKeyId?: string; modelId?: string; fallbackEnabled?: boolean;
      maxRuns?: number; enabled?: boolean;
    }) => cmd('subagent_update', { input: data }),
    delete: (id: string) => cmd('subagent_delete', { id }),
    run: (data: { subagentId: string; inputText: string }) =>
      cmd('subagent_run', { input: data }),
    listRuns: (subagentId: string) =>
      cmd('subagent_list_runs', { subagentId }),
    resolveBinding: (subagentId: string) =>
      cmd('subagent_resolve_binding', { subagentId }),
};

  // Assistant in-process RPC (no daemon sidecar)
export const assistantV2: NativesAPI['assistantV2'] = {
    request: <T>(method: string, params?: unknown): Promise<T> =>
      cmd<AssistantRpcEnvelope<T>>('assistant_rpc_request', { method, params: params ?? null })
        .then(unwrapAssistantRpc),
    getStatus: (): Promise<{ connected: boolean; error: string | null }> =>
      cmd<{ connected: boolean; error: string | null }>('assistant_status'),
    subscribeHarness: (
      listener: (notice: HarnessNotice) => void,
      options?: { cursor?: number; onError?: (error: unknown) => void },
    ): (() => void) => {
      let stopped = false;
      let cursor = options?.cursor ?? 0;
      const run = async () => {
        while (!stopped) {
          try {
            const page = await assistantV2.request<{
              notices: HarnessNotice[];
              next_cursor: number;
              reset_required?: boolean;
            }>('harness.subscribe', { cursor, wait_ms: 25_000, limit: 100 });
            if (stopped) break;
            for (const notice of page.notices) listener(notice);
            cursor = page.next_cursor;
          } catch (error) {
            if (stopped) break;
            options?.onError?.(error);
            await new Promise((resolve) => window.setTimeout(resolve, 1_000));
          }
        }
      };
      void run();
      return () => { stopped = true; };
    },
};

  // Project directory management
export const project: NativesAPI['project'] = {
    list: (): Promise<ProjectSummary[]> => cmd<ProjectSummary[]>('project_list'),
    listHidden: (): Promise<string[]> => cmd<string[]>('project_list_hidden'),
    register: (path: string): Promise<ProjectSummary> => cmd<ProjectSummary>('project_register', { path }),
    rename: (id: string, label: string): Promise<void> => cmd<void>('project_rename', { id, label }),
    remove: (id: string): Promise<void> => cmd<void>('project_remove', { id }),
};

