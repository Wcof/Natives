'use client';

/**
 * Home 文档持久化（兼容层，V2 迁移）。
 *
 * ⚠️ DEPRECATED: 自 SQLite v27 起，Home 数据权威为 `workspaces` /
 * `workspace_widgets` / `workspace_layouts` 表，renderer 只通过 typed IPC
 * （`src/lib/workspace/client.ts`）读写——不再写 `settings:home_workspace`
 * K/V（该 key 由 `migrate_v27` 一次性导入 `home` workspace 后删除）。
 *
 * 本文件保留旧函数签名（`loadHomeWorkspaceDocument` / `createDocumentSaver`）
 * 供既有 UI 平滑过渡：load 优先走 v27 snapshot，save 落到 workspace 表的
 * widget/layout 行。浏览器 dev 模式（无 Tauri bridge）回退读旧 K/V，只读。
 */

import { DEFAULT_DOCUMENT, type HomeWorkspaceDocument, type HomeResponsiveLayouts } from './model';
import type { LayoutItem } from 'react-grid-layout';
import {
  createWorkspace,
  listWorkspaces,
  upsertWidget,
  removeWidget,
  saveLayout,
  getWorkspace,
} from '../workspace/client';
import type { WorkspaceBreakpoint, WorkspaceSnapshot } from '../workspace/contracts';

const LEGACY_STORAGE_KEY = 'settings:home_workspace';
const HOME_WORKSPACE_ID = 'home';
const SAVE_DEBOUNCE_MS = 800;

/** 确保 `home` workspace 存在（v27 导入或新建），返回其 id。 */
async function ensureHomeWorkspace(): Promise<string> {
  const list = await listWorkspaces();
  const home = list.find((w) => w.id === HOME_WORKSPACE_ID || w.kind === 'home');
  if (home) return home.id;
  const created = await createWorkspace({
    name: 'Home',
    kind: 'home',
    theme: 'dark',
  });
  return created.workspace.id;
}

/** v27 snapshot → legacy HomeWorkspaceDocument 形状（只读兼容）。 */
function snapshotToDocument(snapshot: WorkspaceSnapshot): HomeWorkspaceDocument {
  const instances = snapshot.widgets.map((w) => ({
    id: w.id,
    widget_type: w.widgetType,
    config: w.config,
  }));
  const hidden = snapshot.widgets.filter((w) => w.hidden).map((w) => w.id);
  const toBreakpoint = (bp: WorkspaceBreakpoint): LayoutItem[] => {
    const layout = snapshot.layouts.find((l) => l.breakpoint === bp);
    const raw = Array.isArray(layout?.layout) ? layout!.layout : [];
    return raw as LayoutItem[];
  };
  const layouts: HomeResponsiveLayouts = {
    lg: toBreakpoint('lg'),
    md: toBreakpoint('md'),
    sm: toBreakpoint('sm'),
  };
  return { schemaVersion: 1, hidden, instances, layouts };
}

/** 旧 K/V 读取（浏览器 dev / 迁移前兜底，只读）。 */
async function loadLegacyDocument(): Promise<HomeWorkspaceDocument> {
  try {
    const api = window.nativesAPI;
    if (!api?.db?.get) return DEFAULT_DOCUMENT;
    const raw = await api.db.get(LEGACY_STORAGE_KEY);
    if (typeof raw !== 'string' || !raw) return DEFAULT_DOCUMENT;
    const parsed = JSON.parse(raw) as HomeWorkspaceDocument;
    if (parsed.schemaVersion !== 1 || !parsed.instances || !parsed.layouts) {
      return DEFAULT_DOCUMENT;
    }
    return parsed;
  } catch {
    return DEFAULT_DOCUMENT;
  }
}

/** 从持久层加载文档；损坏/未知版本/不可达回退默认（可恢复，不崩溃）。 */
export async function loadHomeWorkspaceDocument(): Promise<HomeWorkspaceDocument> {
  // V2 权威路径：`home` workspace snapshot（migrate_v27 导入过 legacy 文档）。
  try {
    const snapshot = await getWorkspace(HOME_WORKSPACE_ID);
    if (snapshot) return snapshotToDocument(snapshot);
  } catch {
    // 无 bridge（浏览器 dev）→ 走旧 K/V 只读兜底。
  }
  return loadLegacyDocument();
}

/**
 * 生成带防抖的持久化器：连续 stop 事件合并为一次写。
 * 写路径为 workspace 表的 widget/layout 行（typed IPC），不再写 K/V。
 */
export function createDocumentSaver() {
  let timer: ReturnType<typeof setTimeout> | null = null;

  const flush = async (document: HomeWorkspaceDocument) => {
    try {
      const workspaceId = await ensureHomeWorkspace();
      const instanceIds = new Set(document.instances.map((i) => i.id));
      // Sync: remove DB widget rows that are no longer in the document (the
      // legacy `hidden` list keeps deleted widgets out of `instances`).
      const existing = await getWorkspace(workspaceId);
      if (existing) {
        for (const widget of existing.widgets) {
          if (!instanceIds.has(widget.id)) {
            await removeWidget(workspaceId, widget.id);
          }
        }
      }
      // Widget 行（upsert；hidden 由 legacy hidden 列表推导）。
      for (const instance of document.instances) {
        await upsertWidget(workspaceId, {
          id: instance.id,
          widgetType: instance.widget_type,
          config: instance.config,
          hidden: document.hidden.includes(instance.id),
        });
      }
      // Layout 行（每断点 upsert）。
      for (const [bp, layout] of Object.entries(document.layouts)) {
        await saveLayout(workspaceId, bp, JSON.stringify(layout));
      }
    } catch {
      // 浏览器 dev 模式无 IPC；会话内状态不受影响（legacy K/V 只读兜底）
    }
  };

  const schedule = (document: HomeWorkspaceDocument) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void flush(document);
    }, SAVE_DEBOUNCE_MS);
  };

  return { schedule, flush };
}
