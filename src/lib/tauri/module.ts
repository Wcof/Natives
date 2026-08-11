/**
 * tauri/module — 模块（Workshop web-module）域 facade（ARCH-002）
 *
 * 业务组件只允许经本 facade 访问 module 能力；唯一 raw invoke 在
 * ./core.ts（`cmd`）。
 */

import { cmd } from './core';
import type { NativesAPI } from './types';

/** Workshop 模块生命周期 / 权限 / 审计 域命令。 */
const moduleApi: NativesAPI['module'] = {
  scan: () => cmd('module_scan'),
  install: (pathOrZip: string) => cmd('module_install', { pathOrZip }),
  readManifest: (source: string) => cmd('module_read_manifest', { source }),
  grantPermission: (moduleId: string, permission: string) =>
    cmd('module_grant_permission', { moduleId, permission }),
  revokePermission: (moduleId: string, permission: string) =>
    cmd('module_revoke_permission', { moduleId, permission }),
  listPermissions: (moduleId: string) => cmd('module_list_permissions', { moduleId }),
  getAuditLog: (moduleId?: string, limit?: number) =>
    cmd('module_get_audit_log', { moduleId, limit }),
  approveAllPermissions: (moduleId: string) =>
    cmd('module_approve_all_permissions', { moduleId }),
  uninstall: (moduleId: string) => cmd('module_uninstall', { moduleId }),
  list: () => cmd('module_list'),
  enable: (moduleId: string) => cmd('module_enable', { moduleId }),
  disable: (moduleId: string) => cmd('module_disable', { moduleId }),
  update: (moduleId: string, source?: string) =>
    cmd('module_update', { moduleId, source }),
  writeGenerated: (
    moduleId: string,
    name: string,
    htmlContent: string,
    permissions: string[],
  ) =>
    cmd<import('./types').WriteGeneratedModuleResult>('write_generated_module', {
      moduleId,
      name,
      htmlContent,
      permissions,
    }),
  rollback: (params: { moduleId: string; oldContent: string }) =>
    cmd('rollback_module', params),
};

// 公共导出名保持 `module`（兼容既有 import / HMR 契约）。
// 注意：顶层 binding 命名 `moduleApi`，避免遮蔽 Webpack/React Refresh 的
// factory 参数 `module`（`module.hot.data` 读取崩溃的根因）。
export { moduleApi as module };
