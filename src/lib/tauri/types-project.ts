/**
 * tauri/types-project — 工作区 / run 系列共享类型（ARCH-002 split）
 *
 * 项目目录（ProjectSummary）与 assistant harness 通知（HarnessNotice）声明于此；
 * assistant facade（./assistant.ts）与业务组件从这里取类型。
 */

export interface ProjectSummary {
  id: string;
  path: string;
  label: string;
  conversationCount: number;
  exists: boolean;
  lastOpenedAt: string;
}

export interface HarnessNotice {
  cursor: number;
  kind: 'published' | 'binding_changed' | 'source_drift' | 'trace_updated' | 'reset_required';
  profile_id?: string | null;
  run_id?: string | null;
  created_at?: string;
}
