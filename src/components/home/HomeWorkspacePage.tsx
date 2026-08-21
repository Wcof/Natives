'use client';

/**
 * PersonalWorkspace Home（ADR-0020 §3 / Home Patch 决策 1/3/4/5/8）。
 *
 * - `/` 是唯一 Personal Workspace Home；完整 Usage 已迁至数据/用量页。
 * - Grid-based Widget：可拖动/缩放/增删/配置/恢复默认（react-grid-layout）。
 * - 普通模式安静；编辑模式才出现编辑控件（决策 5，临时状态不持久化）。
 * - 布局持久化：单份版本化 JSON 存 settings K/V（决策 8），
 *   拖动/缩放只在 stop 后 debounce 写（ADR-0020 §3，绝不 per-pointer 写 DB）。
 * - 无 Widget Runtime / Plugin / Infinite Canvas / 嵌套容器。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { Responsive, noCompactor, useContainerWidth, type Layout } from 'react-grid-layout';
import { Pencil, Plus, RotateCcw, X } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import {
  BREAKPOINTS,
  COLUMNS,
  DEFAULT_DOCUMENT,
  type HomeBreakpoint,
  type HomeWorkspaceDocument,
  type WidgetInstance,
} from '@/lib/home-workspace/model';
import {
  normalizeResponsiveLayouts,
  nextFreeSlot,
} from '@/lib/home-workspace/layoutModel';
import { WIDGET_REGISTRY, getWidgetDescriptor } from '@/lib/home-workspace/registry';
import { createDocumentSaver, loadHomeWorkspaceDocument } from '@/lib/home-workspace/persistence';
import { getWidgetComponent } from './widgets';

const STABLE_COMPACTOR = { ...noCompactor, preventCollision: true };

function createInstance(widgetType: string, index: number): WidgetInstance {
  return {
    id: `widget-${Date.now().toString(36)}-${index}`,
    widget_type: widgetType,
    config: {},
  };
}

/** 默认布局：5 个核心 Widget（决策 6），自动放置不重叠。 */
function buildDefaultDocument(): HomeWorkspaceDocument {
  const visible = WIDGET_REGISTRY.filter((widget) => widget.defaultVisible);
  const instances = visible.map((widget, index) => createInstance(widget.id, index));
  const layouts = normalizeResponsiveLayouts(undefined, instances.map((instance) => instance.id));
  return { ...DEFAULT_DOCUMENT, instances, layouts };
}

export default function HomeWorkspacePage() {
  const locale = useLocale();
  const [document, setDocument] = useState<HomeWorkspaceDocument>(buildDefaultDocument);
  const [editMode, setEditMode] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [activeBreakpoint, setActiveBreakpoint] = useState<HomeBreakpoint>('lg');
  const saverRef = useRef(createDocumentSaver());
  const documentRef = useRef(document);
  documentRef.current = document;

  const { width, containerRef, mounted } = useContainerWidth({
    measureBeforeMount: true,
    initialWidth: 1000,
  });

  // 首次挂载：从持久层恢复（失败回退默认，可恢复不崩溃）。
  useEffect(() => {
    let cancelled = false;
    void loadHomeWorkspaceDocument().then((loadedDoc) => {
      if (cancelled) return;
      const instanceIds = loadedDoc.instances.map((instance) => instance.id);
      setDocument({
        ...loadedDoc,
        layouts: normalizeResponsiveLayouts(loadedDoc.layouts, instanceIds),
      });
      setLoaded(true);
    });
    return () => { cancelled = true; };
  }, []);

  const scheduleSave = useCallback((next: HomeWorkspaceDocument) => {
    // 拖动/缩放过程中不写 SQLite：只有 stop 后的 debounce 才持久化（ADR-0020 §3）。
    saverRef.current.schedule(next);
  }, []);

  const commitStoppedLayout = useCallback((layout: Layout) => {
    setDocument((current) => {
      const instanceIds = current.instances.map((instance) => instance.id);
      const next = {
        ...current,
        layouts: {
          ...current.layouts,
          [activeBreakpoint]: (layout ?? []).filter((item) => instanceIds.includes(item.i)),
        },
      };
      scheduleSave(next);
      return next;
    });
  }, [activeBreakpoint, scheduleSave]);

  const addWidget = useCallback((widgetType: string) => {
    setDocument((current) => {
      const descriptor = getWidgetDescriptor(widgetType);
      if (!descriptor) return current;
      const instanceIds = [...current.instances.map((instance) => instance.id), 'placeholder'];
      const base = normalizeResponsiveLayouts(current.layouts, instanceIds);
      const layout = base[activeBreakpoint] ?? [];
      const slot = nextFreeSlot(layout, activeBreakpoint, descriptor.defaultSize.w, descriptor.defaultSize.h);
      const instance = createInstance(widgetType, current.instances.length);
      const next: HomeWorkspaceDocument = {
        ...current,
        hidden: current.hidden.filter((id) => id !== widgetType),
        instances: [...current.instances, instance],
        layouts: {
          ...current.layouts,
          [activeBreakpoint]: [
            ...layout,
            { i: instance.id, x: slot.x, y: slot.y, w: descriptor.defaultSize.w, h: descriptor.defaultSize.h, minW: descriptor.minSize.w, minH: descriptor.minSize.h, maxW: descriptor.maxSize.w, maxH: descriptor.maxSize.h, isBounded: true },
          ],
        },
      };
      scheduleSave(next);
      return next;
    });
  }, [activeBreakpoint, scheduleSave]);

  const removeWidget = useCallback((instanceId: string) => {
    setDocument((current) => {
      const instance = current.instances.find((item) => item.id === instanceId);
      const next: HomeWorkspaceDocument = {
        ...current,
        hidden: instance ? [...current.hidden, instance.widget_type] : current.hidden,
        instances: current.instances.filter((item) => item.id !== instanceId),
        layouts: normalizeResponsiveLayouts(
          current.layouts,
          current.instances.filter((item) => item.id !== instanceId).map((item) => item.id),
        ),
      };
      scheduleSave(next);
      return next;
    });
  }, [scheduleSave]);

  const resetDefault = useCallback(() => {
    const next = buildDefaultDocument();
    setDocument(next);
    scheduleSave(next);
  }, [scheduleSave]);

  return (
    <div ref={containerRef} className="flex h-full flex-col overflow-hidden">
      {/* 编辑模式工具栏：临时 UI 状态，不持久化（决策 5） */}
      <div className="flex shrink-0 items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
        <button
          type="button"
          aria-pressed={editMode}
          onClick={() => setEditMode((value) => !value)}
          className={`inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs transition-colors ${
            editMode
              ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
              : 'bg-[var(--surface)] text-[var(--text-secondary)] hover:text-[var(--text)]'
          }`}
        >
          <Pencil size={13} />
          {t(locale, editMode ? 'home.doneEditing' : 'home.edit')}
        </button>
        {editMode && (
          <>
            <button
              type="button"
              onClick={resetDefault}
              className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--surface)] px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:text-[var(--text)]"
            >
              <RotateCcw size={13} />
              {t(locale, 'home.resetDefault')}
            </button>
            <span className="ml-1 text-xs text-[var(--text-disabled)]">{t(locale, 'home.editHint')}</span>
          </>
        )}
      </div>

      <div className="relative flex-1 min-h-0">
        {mounted ? (
          <Responsive<HomeBreakpoint>
            width={width}
            layouts={document.layouts}
            breakpoints={BREAKPOINTS}
            cols={COLUMNS}
            rowHeight={32}
            margin={[12, 12]}
            containerPadding={[16, 16]}
            compactor={STABLE_COMPACTOR}
            dragConfig={{
              enabled: editMode,
              bounded: true,
              handle: '.widget-drag-handle',
              cancel: '.widget-content',
              threshold: 3,
            }}
            resizeConfig={{ enabled: editMode, handles: ['se'] }}
            onBreakpointChange={(breakpoint) => setActiveBreakpoint(breakpoint)}
            onDragStop={commitStoppedLayout}
            onResizeStop={commitStoppedLayout}
          >
            {document.instances.map((instance) => {
              const descriptor = getWidgetDescriptor(instance.widget_type);
              const Component = getWidgetComponent(instance.widget_type);
              if (!descriptor || !Component) return null;
              return (
                <article
                  key={instance.id}
                  data-testid={`home-widget-${instance.widget_type}`}
                  className={`widget-shell relative flex h-full flex-col overflow-hidden rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)] ${editMode ? 'editing' : ''}`}
                >
                  {editMode && (
                    <>
                      <div className="widget-drag-handle absolute inset-x-0 top-0 z-10 flex h-6 cursor-move items-center justify-between bg-[var(--surface-hover)] px-2 text-[var(--text-disabled)]">
                        <span className="text-[0.625rem]">{t(locale, descriptor.titleKey)}</span>
                        <button
                          type="button"
                          onClick={() => removeWidget(instance.id)}
                          aria-label={t(locale, 'home.removeWidget')}
                          className="rounded p-0.5 hover:text-[var(--danger)]"
                        >
                          <X size={12} />
                        </button>
                      </div>
                    </>
                  )}
                  <div className="widget-content min-h-0 flex-1 overflow-hidden px-3 pb-3 pt-1">
                    <Component />
                  </div>
                </article>
              );
            })}
          </Responsive>
        ) : (
          <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
            {t(locale, 'common.loading')}
          </div>
        )}

        {/* Widget Picker：仅编辑模式显示 */}
        {editMode && (
          <div className="absolute right-3 top-3 z-20 flex flex-wrap gap-1.5 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)] p-2 shadow-lg">
            {WIDGET_REGISTRY.filter(
              (widget) => !document.instances.some((instance) => instance.widget_type === widget.id),
            ).map((widget) => (
              <button
                key={widget.id}
                type="button"
                onClick={() => addWidget(widget.id)}
                className="inline-flex items-center gap-1 rounded-lg bg-[var(--surface-hover)] px-2.5 py-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text)]"
              >
                <Plus size={12} />
                {t(locale, widget.titleKey)}
              </button>
            ))}
          </div>
        )}
      </div>
      <span className="sr-only" data-loaded={loaded ? 'true' : 'false'} />
    </div>
  );
}
