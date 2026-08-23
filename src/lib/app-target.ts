/**
 * app-target — Shell 当前应用呈现目标的最小状态机（APPV2-T02）。
 *
 * 设计约束（应用中心 V2 方案）：
 * - 状态只活在 ShellLayout（组件局部 state），**不引入全局 store / 新 Event Bus**。
 * - 侧边栏点击 = 先解析 AppView → `switching` → 打开成功 `presented` / 失败 `error`；
 *   失败保留可恢复错误（重试 / 返回应用中心），不静默回滚。
 * - `local_project` 在 UI 层呈现为 unsupported（T08 切割产品入口；数据与底层能力保留）。
 * - 纯函数 reducer：状态转换可单测，副作用（IPC open/webHide）留在 ShellLayout。
 */

import type { AppKind, AppView } from './tauri/apps';

export type AppTargetPhase = 'switching' | 'presented' | 'error';

/** 应用中心 unsupported 的本地项目类型标记（错误文案键，非自由文本）。 */
export const UNLOCAL_PROJECT_ERROR = 'unsupported_local_project';

export interface ActiveAppTarget {
  appId: string;
  /** 解析前的临时 kind（view 到达后以 view.kind 为准）。 */
  kind: AppKind;
  /** AppView 解析结果；switching 早期为 null。 */
  view: AppView | null;
  phase: AppTargetPhase;
  /** error 阶段的用户可恢复错误信息（已本地化）。 */
  error?: string | null;
}

export type AppTargetAction =
  | { type: 'request'; appId: string }
  | { type: 'viewResolved'; appId: string; view: AppView }
  | { type: 'presented'; appId: string }
  | { type: 'failed'; appId: string; error: string }
  | { type: 'cleared' };

export function reduceAppTarget(
  prev: ActiveAppTarget | null,
  action: AppTargetAction,
): ActiveAppTarget | null {
  switch (action.type) {
    case 'request': {
      // 重新请求同一目标（例如从 error 恢复）时保留已解析的 view。
      const keptView = prev?.appId === action.appId ? prev.view : null;
      return {
        appId: action.appId,
        kind: keptView ? (keptView.kind ?? 'web_application') : 'web_application',
        view: keptView,
        phase: 'switching',
        error: null,
      };
    }
    case 'viewResolved': {
      // appId 不匹配 = 过期响应（竞态），忽略；token 保护由调用方负责副作用。
      if (!prev || prev.appId !== action.appId) return prev;
      if (action.view.kind === 'local_project') {
        // UI 层 unsupported：保留 view 供展示标题，进入 error 阶段。
        return {
          appId: action.appId,
          kind: action.view.kind,
          view: action.view,
          phase: 'error',
          error: UNLOCAL_PROJECT_ERROR,
        };
      }
      return { ...prev, kind: action.view.kind, view: action.view, phase: 'switching', error: null };
    }
    case 'presented': {
      if (!prev || prev.appId !== action.appId) return prev;
      return { ...prev, phase: 'presented', error: null };
    }
    case 'failed': {
      // 过期 failed（目标已切换）忽略，避免旧目标的错误覆盖新目标。
      if (prev && prev.appId !== action.appId) return prev;
      if (prev) {
        return { ...prev, phase: 'error', error: action.error };
      }
      return { appId: action.appId, kind: 'web_application', view: null, phase: 'error', error: action.error };
    }
    case 'cleared':
      return null;
  }
}

/** 目标是否仍呈现（可用于「切离时只 hide 受管目标」判定）。 */
export function isWebTargetPresented(
  target: ActiveAppTarget | null,
): target is ActiveAppTarget & { kind: 'web_application' } {
  return !!target && target.kind === 'web_application' && (target.phase === 'presented' || target.phase === 'switching');
}
