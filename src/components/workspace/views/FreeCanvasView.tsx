'use client';

/**
 * FreeCanvasView (C-012..C-022) — self-built lightweight DOM canvas.
 *
 * Interaction model:
 *  - camera pan (hand tool / space+drag) and zoom (wheel / toolbar / fit).
 *  - node drag, 8-handle resize, click / shift-click / marquee selection.
 *  - snap to the 16px grid and to other nodes' edges (8px tolerance).
 *  - groups & frames (a group is a container node whose members move with it).
 *  - z-order (bring to front / send to back / nudges).
 *
 * Performance contract: pointer moves mutate ONLY an in-memory draft
 * (memory-only); the committed node list is updated on pointer-up and
 * persisted through a debounced onCommit. No per-pointer persistence.
 *
 * Inspired by common infinite-canvas concepts; no AFFiNE / Plane / Twenty /
 * Yjs / CRDT / BlockSuite source is copied.
 */

import { useLocale, t } from '@/i18n';
import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  forwardRef,
} from 'react';
import {
  Frame,
  Hand,
  MousePointer2,
  SendToBack,
  BringToFront,
  Minus,
  Plus,
  Maximize,
  Layers,
  StickyNote,
  Trash2,
  Ungroup,
  LayoutGrid,
} from 'lucide-react';
import {
  CANVAS_GRID,
  CANVAS_MAX_ZOOM,
  CANVAS_MIN_ZOOM,
  createCanvasNode,
  type CanvasCamera,
  type CanvasNode,
  type CanvasNodeKind,
  type CanvasRect,
  type CanvasSelection,
} from '@/lib/workspace/canvas/types';
import {
  bringToFront,
  CANVAS_WORLD,
  clamp,
  hitTestNodes,
  nextZ,
  nodesInMarquee,
  normalizeRect,
  sendToBack,
  snap,
} from '@/lib/workspace/canvas/geometry';
import { clampCamera, fitCameraToWorld, screenToWorld, zoomStep } from '@/lib/workspace/canvas/camera';
import { AddWidgetMenu } from '../widgets/AddWidgetMenu';
import { WidgetRenderer } from '../widgets/WidgetRenderer';
import {
  getWidget,
  normalizeWidgetConfig,
  createDefaultConfig,
} from '@/lib/workspace/widgets';

type Tool = 'select' | 'hand';
type HandleKey =
  | 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

interface Gesture {
  mode: 'pan' | 'drag' | 'resize' | 'marquee';
  startScreenX: number;
  startScreenY: number;
  startCamera: CanvasCamera;
  nodeId?: string;
  handle?: HandleKey;
  dragStart: { x: number; y: number };
  marqueeStart: { x: number; y: number };
}

interface FreeCanvasViewProps {
  initialNodes: CanvasNode[];
  onCommit: (nodes: CanvasNode[]) => void;
  editable?: boolean;
  onSelectionChange?: (ids: string[]) => void;
}

export interface FreeCanvasViewHandle {
  deleteSelection: () => void;
  groupSelection: () => void;
  ungroupSelection: () => void;
  bringToFront: () => void;
  sendToBack: () => void;
}

const HANDLES: HandleKey[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

const HANDLE_CURSOR: Record<HandleKey, string> = {
  nw: 'nwse-resize',
  se: 'nwse-resize',
  ne: 'nesw-resize',
  sw: 'nesw-resize',
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
};

export default forwardRef<FreeCanvasViewHandle, FreeCanvasViewProps>(function FreeCanvasView(
  { initialNodes, onCommit, editable = true, onSelectionChange }: FreeCanvasViewProps,
  ref,
) {
  const locale = useLocale();
  const [nodes, setNodes] = useState<CanvasNode[]>(() => initialNodes);
  const [draft, setDraft] = useState<Record<string, Partial<CanvasNode>>>({});
  const [camera, setCamera] = useState<CanvasCamera>({ x: 300, y: 200, zoom: 1 });
  const [selection, setSelection] = useState<CanvasSelection>({ ids: [], mode: 'none' });
  const [tool, setTool] = useState<Tool>('select');
  const [marquee, setMarquee] = useState<CanvasRect | null>(null);
  const [viewport, setViewport] = useState({ w: 800, h: 600 });
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const addWidgetButtonRef = useRef<HTMLButtonElement | null>(null);

  const stageRef = useRef<HTMLDivElement | null>(null);
  const gestureRef = useRef<Gesture | null>(null);
  const cameraRef = useRef(camera);
  cameraRef.current = camera;
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const spaceRef = useRef(false);
  const commitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onCommitRef = useRef(onCommit);
  onCommitRef.current = onCommit;
  const onSelectionChangeRef = useRef(onSelectionChange);
  onSelectionChangeRef.current = onSelectionChange;
  const selectionRef = useRef(selection);
  selectionRef.current = selection;

  /** Set selection and report it upward (for the Inspector host). */
  const updateSelection = useCallback((next: CanvasSelection) => {
    setSelection(next);
    onSelectionChangeRef.current?.(next.ids);
  }, []);

  // Track the stage size for coordinate transforms.
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const updateSize = () => {
      const rect = stage.getBoundingClientRect();
      setViewport({ w: rect.width, h: rect.height });
    };
    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(stage);
    return () => observer.disconnect();
  }, []);

  // Space toggles the hand tool while held.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.code === 'Space' && !e.repeat) spaceRef.current = true;
    };
    const up = (e: KeyboardEvent) => {
      if (e.code === 'Space') spaceRef.current = false;
    };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
    };
  }, []);

  const commitSoon = useCallback((next: CanvasNode[]) => {
    setNodes(next);
    if (commitTimerRef.current) clearTimeout(commitTimerRef.current);
    commitTimerRef.current = setTimeout(() => {
      onCommitRef.current(next);
      commitTimerRef.current = null;
    }, 120);
  }, []);

  const resetDraft = useCallback(() => setDraft({}), []);

  const worldAt = useCallback(
    (clientX: number, clientY: number) => {
      const stage = stageRef.current;
      if (!stage) return { x: 0, y: 0 };
      const rect = stage.getBoundingClientRect();
      return screenToWorld(
        { x: clientX - rect.left, y: clientY - rect.top },
        cameraRef.current,
        { x: viewport.w, y: viewport.h },
      );
    },
    [viewport],
  );

  const beginGesture = useCallback(
    (e: React.PointerEvent) => {
      if (!editable) return;
      if (e.button !== 0) return;
      try {
        stageRef.current?.setPointerCapture(e.pointerId);
      } catch {
        // capture unsupported — continue with window-level move tracking
      }
      const point = worldAt(e.clientX, e.clientY);
      const usePan = tool === 'hand' || spaceRef.current;

      if (usePan) {
        gestureRef.current = {
          mode: 'pan',
          startScreenX: e.clientX,
          startScreenY: e.clientY,
          startCamera: cameraRef.current,
          dragStart: point,
          marqueeStart: point,
        };
        return;
      }

      const hit = hitTestNodes(nodesRef.current, point.x, point.y);
      if (hit) {
        const ids = selectionRef.current.ids.includes(hit.id)
          ? selectionRef.current.ids
          : [hit.id];
        updateSelection({ ids, mode: 'single' });
        gestureRef.current = {
          mode: 'drag',
          startScreenX: e.clientX,
          startScreenY: e.clientY,
          startCamera: cameraRef.current,
          nodeId: hit.id,
          dragStart: { x: hit.x, y: hit.y },
          marqueeStart: point,
        };
        return;
      }

      // Empty space → marquee select.
      updateSelection({ ids: [], mode: 'marquee' });
      setMarquee({ x: point.x, y: point.y, w: 0, h: 0 });
      gestureRef.current = {
        mode: 'marquee',
        startScreenX: e.clientX,
        startScreenY: e.clientY,
        startCamera: cameraRef.current,
        dragStart: point,
        marqueeStart: point,
      };
    },
    [editable, tool, viewport, worldAt],
  );

  const beginResize = useCallback(
    (e: React.PointerEvent, nodeId: string, handle: HandleKey) => {
      if (!editable) return;
      e.stopPropagation();
      try {
        stageRef.current?.setPointerCapture(e.pointerId);
      } catch {
        // capture unsupported
      }
      const node = nodesRef.current.find((item) => item.id === nodeId);
      if (!node || node.locked) return;
      updateSelection({ ids: [nodeId], mode: 'single' });
      gestureRef.current = {
        mode: 'resize',
        startScreenX: e.clientX,
        startScreenY: e.clientY,
        startCamera: cameraRef.current,
        nodeId,
        handle,
        dragStart: { x: node.x, y: node.y },
        marqueeStart: { x: node.x, y: node.y },
      };
    },
    [editable],
  );

  const moveGesture = useCallback(
    (e: React.PointerEvent) => {
      const gesture = gestureRef.current;
      if (!gesture) return;
      const world = worldAt(e.clientX, e.clientY);
      const dxWorld = (e.clientX - gesture.startScreenX) / cameraRef.current.zoom;
      const dyWorld = (e.clientY - gesture.startScreenY) / cameraRef.current.zoom;

      if (gesture.mode === 'pan') {
        setCamera(
          clampCamera({
            ...gesture.startCamera,
            x: gesture.startCamera.x - dxWorld,
            y: gesture.startCamera.y - dyWorld,
          }),
        );
        return;
      }

      if (gesture.mode === 'drag') {
        const nextDraft: Record<string, Partial<CanvasNode>> = {};
        for (const id of selectionRef.current.ids) {
          const node = nodesRef.current.find((item) => item.id === id);
          if (!node || node.locked) continue;
          const nx = snap(node.x + dxWorld);
          const ny = snap(node.y + dyWorld);
          nextDraft[id] = { x: nx, y: ny };
          if (node.kind === 'group' && node.members) {
            for (const memberId of node.members) {
              const member = nodesRef.current.find((item) => item.id === memberId);
              if (member) {
                nextDraft[memberId] = {
                  x: snap(member.x + dxWorld),
                  y: snap(member.y + dyWorld),
                };
              }
            }
          }
        }
        setDraft(nextDraft);
        return;
      }

      if (gesture.mode === 'resize' && gesture.nodeId) {
        const node = nodesRef.current.find((item) => item.id === gesture.nodeId);
        if (!node || node.locked) return;
        const start = { x: node.x, y: node.y, w: node.w, h: node.h };
        const next = resizeFromHandle(start, gesture.handle!, dxWorld, dyWorld);
        const snapped = {
          x: gesture.handle?.includes('w') ? snap(next.x) : start.x,
          y: gesture.handle?.includes('n') ? snap(next.y) : start.y,
          w: snap(next.w),
          h: snap(next.h),
        };
        setDraft({ [node.id]: { x: snapped.x, y: snapped.y, w: snapped.w, h: snapped.h } });
        return;
      }

      if (gesture.mode === 'marquee') {
        const rect = normalizeRect(gesture.marqueeStart.x, gesture.marqueeStart.y, world.x - gesture.marqueeStart.x, world.y - gesture.marqueeStart.y);
        setMarquee(rect);
        updateSelection({
          ids: nodesInMarquee(nodesRef.current, rect).map((node) => node.id),
          mode: 'marquee',
        });
      }
    },
    [worldAt],
  );

  const endGesture = useCallback((e?: React.PointerEvent) => {
    const gesture = gestureRef.current;
    gestureRef.current = null;
    setMarquee(null);
    if (e && stageRef.current) {
      try { stageRef.current.releasePointerCapture(e.pointerId); } catch { /* ok */ }
    }
    if (gesture?.mode === 'drag' || gesture?.mode === 'resize') {
      const overrides = draftRef.current;
      const next = nodesRef.current.map((node) =>
        overrides[node.id] ? { ...node, ...overrides[node.id] } : node,
      );
      resetDraft();
      setNodes(next);
      onCommitRef.current(next);
    }
  }, [resetDraft]);

  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        const factor = e.deltaY < 0 ? 1 + 0.12 : 1 - 0.12;
        const stage = stageRef.current;
        if (!stage) return;
        const rect = stage.getBoundingClientRect();
        const screen = { x: e.clientX - rect.left, y: e.clientY - rect.top };
        setCamera((c) => clampCamera(zoomAtCursor(c, screen, factor)));
      } else {
        setCamera((c) =>
          clampCamera({ ...c, x: c.x + e.deltaX / c.zoom, y: c.y + e.deltaY / c.zoom }),
        );
      }
    },
    [],
  );

    const zoomAtCursor = (camera: CanvasCamera, screen: { x: number; y: number }, factor: number): CanvasCamera => {
    const before = screenToWorld(screen, camera, { x: viewport.w, y: viewport.h });
    const zoom = clamp(camera.zoom * factor, CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);
    const next = { ...camera, zoom };
    next.x = before.x - (screen.x - viewport.w / 2) / zoom;
    next.y = before.y - (screen.y - viewport.h / 2) / zoom;
    return next;
  };

  const addNode = useCallback(
    (kind: CanvasNodeKind) => {
      const count = nodesRef.current.length;
      const label =
        kind === 'frame'
          ? t(locale, 'workspace.canvasNodeFrameLabel')
          : kind === 'note'
            ? t(locale, 'workspace.canvasNodeNoteLabel')
            : t(locale, 'workspace.canvasNodeLabel', { n: count + 1 });
      const node = createCanvasNode({
        id: `node-${Date.now().toString(36)}-${count}`,
        kind,
        label,
        x: 60 + (count % 5) * 40,
        y: 60 + (count % 4) * 40,
        z: nextZ(nodesRef.current),
        accent: kind === 'note' ? '--primary-soft' : kind === 'frame' ? '--surface-hover' : '--surface',
      });
      commitSoon([...nodesRef.current, node]);
      updateSelection({ ids: [node.id], mode: 'single' });
    },
    [commitSoon, locale, updateSelection],
  );

  const handleAddWidget = useCallback(
    (widgetType: string) => {
      const def = getWidget(widgetType);
      const count = nodesRef.current.length;
      const label = def?.titleKey ? t(locale, def.titleKey) : widgetType;
      const minW = def?.size === 'large' ? 360 : def?.size === 'medium' ? 300 : 260;
      const minH = def?.size === 'large' ? 240 : def?.size === 'medium' ? 180 : 160;
      const node = createCanvasNode({
        id: `widget-${Date.now().toString(36)}-${count}`,
        kind: 'card',
        widgetType,
        label,
        w: minW,
        h: minH,
        x: 60 + (count % 5) * 40,
        y: 60 + (count % 4) * 40,
        z: nextZ(nodesRef.current),
      });
      commitSoon([...nodesRef.current, node]);
      updateSelection({ ids: [node.id], mode: 'single' });
    },
    [commitSoon, locale, updateSelection],
  );

  const groupSelected = useCallback(() => {
    const ids = selectionRef.current.ids;
    const members = nodesRef.current.filter((node) => ids.includes(node.id));
    if (members.length < 2) return;
    const minX = Math.min(...members.map((m) => m.x));
    const minY = Math.min(...members.map((m) => m.y));
    const maxX = Math.max(...members.map((m) => m.x + m.w));
    const maxY = Math.max(...members.map((m) => m.y + m.h));
    const group = createCanvasNode({
      id: `group-${Date.now().toString(36)}`,
      kind: 'group',
      label: t(locale, 'workspace.canvasGroupLabel', { n: nodesRef.current.length + 1 }),
      x: minX - 8,
      y: minY - 24,
      w: maxX - minX + 16,
      h: maxY - minY + 32,
      z: nextZ(nodesRef.current),
      members: members.map((m) => m.id),
      frameId: undefined,
    });
    commitSoon([...nodesRef.current, group]);
    updateSelection({ ids: [group.id], mode: 'single' });
  }, [commitSoon, locale]);

  const ungroupSelected = useCallback(() => {
    const group = nodesRef.current.find(
      (node) => node.kind === 'group' && selectionRef.current.ids.includes(node.id),
    );
    if (!group) return;
    commitSoon(nodesRef.current.filter((node) => node.id !== group.id));
    updateSelection({ ids: group.members ?? [], mode: 'single' });
  }, [commitSoon]);

  const deleteSelected = useCallback(() => {
    const ids = selectionRef.current.ids;
    if (ids.length === 0) return;
    const remaining = nodesRef.current.filter((node) => !ids.includes(node.id));
    commitSoon(remaining);
    updateSelection({ ids: [], mode: 'none' });
  }, [commitSoon]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const ids = selectionRef.current.ids;
      if (ids.length === 0) return;
      if (e.key === 'Delete' || e.key === 'Backspace') {
        e.preventDefault();
        deleteSelected();
      } else if (e.key === 'Escape') {
        updateSelection({ ids: [], mode: 'none' });
      } else if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'ArrowUp' || e.key === 'ArrowDown') {
        if (!selectionRef.current.ids.length) return;
        e.preventDefault();
        const dx = e.shiftKey ? 10 : CANVAS_GRID;
        const deltas = {
          ArrowLeft: { x: -dx, y: 0 },
          ArrowRight: { x: dx, y: 0 },
          ArrowUp: { x: 0, y: -dx },
          ArrowDown: { x: 0, y: dx },
        }[e.key];
        const next = nodesRef.current.map((node) =>
          ids.includes(node.id) && !node.locked
            ? { ...node, x: snap(node.x + deltas.x), y: snap(node.y + deltas.y) }
            : node,
        );
        setNodes(next);
        onCommitRef.current(next);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'g') {
        e.preventDefault();
        if (e.shiftKey) ungroupSelected();
        else groupSelected();
      } else if ((e.metaKey || e.ctrlKey) && e.key === ']') {
        e.preventDefault();
        commitSoon(bringToFront(nodesRef.current, ids));
      } else if ((e.metaKey || e.ctrlKey) && e.key === '[') {
        e.preventDefault();
        commitSoon(sendToBack(nodesRef.current, ids));
      }
    },
    [commitSoon, deleteSelected, groupSelected, ungroupSelected],
  );

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  // Imperative surface for the Inspector host (C-023..C-026).
  useImperativeHandle(
    ref,
    () => ({
      deleteSelection: () => deleteSelected(),
      groupSelection: () => groupSelected(),
      ungroupSelection: () => ungroupSelected(),
      bringToFront: () => commitSoon(bringToFront(nodesRef.current, selectionRef.current.ids)),
      sendToBack: () => commitSoon(sendToBack(nodesRef.current, selectionRef.current.ids)),
    }),
    [commitSoon, deleteSelected, groupSelected, ungroupSelected],
  );

  useEffect(() => {
    return () => {
      if (commitTimerRef.current) clearTimeout(commitTimerRef.current);
    };
  }, []);

  const zoomIn = () =>
    setCamera((c) => zoomStep(clampCamera(c), 1, { x: viewport.w, y: viewport.h }, { x: viewport.w / 2, y: viewport.h / 2 }));
  const zoomOut = () =>
    setCamera((c) => zoomStep(clampCamera(c), -1, { x: viewport.w, y: viewport.h }, { x: viewport.w / 2, y: viewport.h / 2 }));
  const zoomFit = () => setCamera(fitCameraToWorld(CANVAS_WORLD.w, CANVAS_WORLD.h, { x: viewport.w, y: viewport.h }));

  const _selectedNodes = useMemo(
    () => nodes.filter((node) => selection.ids.includes(node.id)),
    [nodes, selection.ids],
  );

  const renderNode = (node: CanvasNode) => {
    const override = draft[node.id] ?? {};
    const rect = { ...node, ...override } as CanvasRect & CanvasNode;
    const selected = selection.ids.includes(node.id);
    const widgetDef = node.widgetType ? getWidget(node.widgetType) : null;
    const widgetConfig = widgetDef
      ? normalizeWidgetConfig(widgetDef, {
          ...createDefaultConfig(widgetDef),
          ...(node.widgetConfig ?? {}),
          order: 0,
        })
      : null;

    return (
      <div
        key={node.id}
        data-node-id={node.id}
        data-testid={`canvas-node-${node.kind}`}
        onPointerDown={(e) => beginGesture(e)}
        className={`absolute touch-none select-none rounded-xl border ${
          node.kind === 'frame'
            ? 'border-dashed border-[var(--border)] bg-[var(--surface-hover)]/40'
            : node.kind === 'group'
              ? 'border-[var(--primary)]/40 bg-[var(--primary-soft)]/20'
              : 'border-[var(--border-subtle)] bg-[var(--surface)] shadow-sm'
        } ${selected ? 'outline-2 outline-[var(--primary)]' : ''} ${node.locked ? 'opacity-70' : ''}`}
        style={{
          left: rect.x,
          top: rect.y,
          width: rect.w,
          height: rect.h,
          zIndex: node.z,
          cursor: tool === 'hand' ? 'grab' : selected ? 'move' : 'default',
        }}
      >
        {widgetDef && widgetConfig ? (
          <div className="flex h-full w-full flex-col overflow-hidden rounded-xl">
            <WidgetRenderer
              instance={{ def: widgetDef, config: widgetConfig }}
              editing={editable}
            />
          </div>
        ) : (
          <>
            <div className="flex h-6 items-center gap-1 border-b border-[var(--border-subtle)] px-2">
              <span className="min-w-0 flex-1 truncate text-[0.625rem] text-[var(--text-secondary)]">
                {node.label}
              </span>
              {node.locked && <span className="text-[0.625rem] text-[var(--text-disabled)]">🔒</span>}
            </div>
            <div className="min-h-0 flex-1 px-2 py-1.5 text-[0.6875rem] leading-relaxed text-[var(--text-secondary)]">
              {node.kind === 'frame' || node.kind === 'group'
                ? t(locale, 'workspace.canvasNodeItems', { count: node.members?.length ?? 0 })
                : t(locale, 'workspace.canvasDoubleClickHint')}
            </div>
          </>
        )}
        {editable && selected && !node.locked && (
          <>
            {HANDLES.map((handle) => {
              const pos = handlePosition(rect, handle);
              return (
                <div
                  key={handle}
                  data-handle={handle}
                  onPointerDown={(e) => beginResize(e, node.id, handle)}
                  className="absolute z-10 h-2 w-2 rounded-sm border border-[var(--border)] bg-[var(--surface)]"
                  style={{
                    left: pos.x,
                    top: pos.y,
                    cursor: HANDLE_CURSOR[handle],
                    transform: 'translate(-50%, -50%)',
                  }}
                />
              );
            })}
          </>
        )}
      </div>
    );
  };

  const activeTool = spaceRef.current ? 'hand' : tool;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      {editable && <div className="flex h-10 shrink-0 items-center gap-1 border-b border-[var(--border-subtle)] px-2">
        <ToolButton active={activeTool === 'select'} onClick={() => setTool('select')} title={t(locale, 'workspace.canvasSelect')} icon={<MousePointer2 size={14} />} />
        <ToolButton active={activeTool === 'hand'} onClick={() => setTool('hand')} title={t(locale, 'workspace.canvasPan')} icon={<Hand size={14} />} />
        <span className="mx-1 h-4 w-px bg-[var(--border-subtle)]" />
        <div className="relative">
          <button
            ref={addWidgetButtonRef}
            type="button"
            aria-haspopup="menu"
            aria-expanded={addMenuOpen}
            onClick={() => setAddMenuOpen((open) => !open)}
            title={t(locale, 'workspace.addCard')}
            aria-label={t(locale, 'workspace.addCard')}
            className={`flex h-7 items-center gap-1 rounded-md px-2 text-xs transition-colors ${
              addMenuOpen
                ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
            }`}
          >
            <LayoutGrid size={13} />
            <span className="text-[0.6875rem] font-medium">{t(locale, 'workspace.addCard')}</span>
          </button>
          <AddWidgetMenu
            open={addMenuOpen}
            onOpenChange={setAddMenuOpen}
            triggerRef={addWidgetButtonRef}
            onSelect={handleAddWidget}
          />
        </div>
        <ToolButton onClick={() => addNode('note')} title={t(locale, 'workspace.canvasAddNote')} icon={<StickyNote size={14} />} />
        <ToolButton onClick={() => addNode('frame')} title={t(locale, 'workspace.canvasAddFrame')} icon={<Frame size={14} />} />
        {selection.ids.length > 0 && (
          <>
            <span className="mx-1 h-4 w-px bg-[var(--border-subtle)]" />
            <ToolButton onClick={groupSelected} title={t(locale, 'workspace.canvasGroup')} icon={<Layers size={14} />} />
            <ToolButton onClick={ungroupSelected} title={t(locale, 'workspace.canvasUngroup')} icon={<Ungroup size={14} />} />
            <ToolButton onClick={() => commitSoon(bringToFront(nodesRef.current, selection.ids))} title={t(locale, 'workspace.canvasFront')} icon={<BringToFront size={14} />} />
            <ToolButton onClick={() => commitSoon(sendToBack(nodesRef.current, selection.ids))} title={t(locale, 'workspace.canvasBack')} icon={<SendToBack size={14} />} />
            <ToolButton onClick={deleteSelected} title={t(locale, 'workspace.canvasDelete')} danger icon={<Trash2 size={14} />} />
          </>
        )}
        <div className="ml-auto flex items-center gap-0.5">
          <ToolButton onClick={zoomOut} title={t(locale, 'workspace.canvasZoomOut')} icon={<Minus size={14} />} />
          <span className="min-w-9 text-center text-[0.625rem] tabular-nums text-[var(--text-disabled)]">
            {Math.round(camera.zoom * 100)}%
          </span>
          <ToolButton onClick={zoomIn} title={t(locale, 'workspace.canvasZoomIn')} icon={<Plus size={14} />} />
          <ToolButton onClick={zoomFit} title={t(locale, 'workspace.canvasFit')} icon={<Maximize size={14} />} />
        </div>
      </div>}

      <div
        ref={stageRef}
        className="relative min-h-0 flex-1 overflow-hidden bg-[var(--surface-subtle)]"
        onPointerDown={beginGesture}
        onPointerMove={moveGesture}
        onPointerUp={endGesture}
        onPointerCancel={endGesture}
        onWheel={handleWheel}
        style={{ cursor: activeTool === 'hand' ? 'grab' : 'default', touchAction: 'none' }}
        data-tool={activeTool}
      >
        <div
          className="absolute origin-top-left"
          style={{
            left: viewport.w / 2 - camera.x * camera.zoom,
            top: viewport.h / 2 - camera.y * camera.zoom,
            width: CANVAS_WORLD.w,
            height: CANVAS_WORLD.h,
            transform: `scale(${camera.zoom})`,
            backgroundImage: editable ? 'repeating-linear-gradient(0deg, var(--border-subtle) 0 1px, transparent 1px 16px), repeating-linear-gradient(90deg, var(--border-subtle) 0 1px, transparent 1px 16px)' : 'none',
          }}
        >
          {nodes.map(renderNode)}
        </div>
        {marquee && (
          <div
            className="pointer-events-none absolute border border-[var(--primary)] bg-[var(--primary-soft)]/25"
            style={{
              left: viewport.w / 2 - camera.x * camera.zoom + marquee.x * camera.zoom,
              top: viewport.h / 2 - camera.y * camera.zoom + marquee.y * camera.zoom,
              width: marquee.w * camera.zoom,
              height: marquee.h * camera.zoom,
            }}
          />
        )}
      </div>
    </div>
  );
});

function ToolButton({
  onClick,
  title,
  icon,
  active,
  danger,
}: {
  onClick: () => void;
  title: string;
  icon: React.ReactNode;
  active?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      className={`flex h-7 w-7 items-center justify-center rounded-md transition-colors ${
        active
          ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
          : danger
            ? 'text-[var(--text-secondary)] hover:bg-[var(--danger)]/10 hover:text-[var(--danger)]'
            : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
      }`}
    >
      {icon}
    </button>
  );
}

function resizeFromHandle(
  start: CanvasRect,
  handle: HandleKey,
  dx: number,
  dy: number,
): CanvasRect {
  let { x, y, w, h } = start;
  if (handle.includes('e')) w = start.w + dx;
  if (handle.includes('s')) h = start.h + dy;
  if (handle.includes('w')) {
    w = start.w - dx;
    x = start.x + dx;
  }
  if (handle.includes('n')) {
    h = start.h - dy;
    y = start.y + dy;
  }
  const min = 48;
  if (w < min) {
    if (handle.includes('w')) x = start.x + start.w - min;
    w = min;
  }
  if (h < min) {
    if (handle.includes('n')) y = start.y + start.h - min;
    h = min;
  }
  return { x, y, w, h };
}

function handlePosition(rect: CanvasRect, handle: HandleKey): { x: number; y: number } {
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const x = handle.includes('e') ? rect.x + rect.w : handle.includes('w') ? rect.x : cx;
  const y = handle.includes('s') ? rect.y + rect.h : handle.includes('n') ? rect.y : cy;
  return { x, y };
}
