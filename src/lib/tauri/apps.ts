/**
 * tauri/apps — Apps domain facade（ADR-0020 / APP-001..018）。
 */

import { cmd } from './core';

export interface App {
  id: string;
  source: string;
  sourceId: string;
  title: string;
  description?: string;
  icon?: string;
  version: string;
  createdAt: string;
  updatedAt: string;
}

export interface RuntimeInstance {
  id: string;
  applicationId: string;
  planId?: string;
  status: string;
  cleanupStatus?: string;
  ownerKind: string;
  pgid?: number;
  currentPort?: number;
  pid?: number;
  failure?: string;
  createdAt: string;
  updatedAt: string;
}

export interface Surface {
  id: string;
  applicationId: string;
  kind: string;
  label: string;
  title?: string;
  url?: string;
  boundsJson?: string;
  createdAt: string;
  updatedAt: string;
}

export interface AppHealthResult {
  healthy: boolean;
  status: string;
  port: number | null;
}

export const appsApi = {
  list: () => cmd<App[]>('apps_list'),
  get: (id: string) => cmd<App | null>('apps_get', { id }),
  create: (input: { title: string; source: string; sourceId: string; description?: string; icon?: string }) =>
    cmd<App>('apps_create', { input }),
  delete: (id: string) => cmd<boolean>('apps_delete', { id }),
  listInstances: (applicationId: string) => cmd<RuntimeInstance[]>('apps_list_instances', { applicationId }),
  listSurfaces: (applicationId: string) => cmd<Surface[]>('apps_list_surfaces', { applicationId }),
  start: (id: string) => cmd<boolean>('apps_start', { id }),
  stop: (id: string) => cmd<boolean>('apps_stop', { id }),
  restart: (id: string) => cmd<boolean>('apps_restart', { id }),
  kill: (id: string) => cmd<boolean>('apps_kill', { id }),
  health: (applicationId: string) => cmd<AppHealthResult>('apps_health', { applicationId }),
};
