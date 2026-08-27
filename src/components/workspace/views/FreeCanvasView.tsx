'use client';

/**
 * FreeCanvasView (WS-03 / WS-04) — view container for the Free Canvas.
 *
 * Responsibilities (each split into a semantic submodule):
 *  - gesture ownership   → `canvas/gesture-controller` (pure)
 *  - node rendering      → `canvas/NodeSurface`
 *  - screen-space resize → `canvas/ResizeOverlay`
 *  - toolbar             → `canvas/Toolbar`
 *  - resize constraints  → `lib/workspace/canvas/constraints`
 *
 * The Stage is the single pointer owner: it captures the pointer on every
 * pointer-down and every `pointermove` updates ONLY the in-memory draft
 * (R-P11 / WS-04: pointermove IPC/DB writes = 0). The committed node list is
 * replaced exactly once on a valid pointer-up (or keyboard stop) and rolled
 * back on pointercancel / Escape. Delete/Backspace/Escape are focus-scoped so
 * typing in an input never deletes canvas nodes.
 */

import { useCallback, useEffect, useMemo, useRef, useState, forwardRef, useImperativeHandle } from 'react';
import { t, useLocale } from '@/i18n';
import {
  type CanvasCamera,
  type CanvasNode,
  type CanvasNodeKind,
  type CanvasSelection,
  type CanvasRect,
  createCanvasNode,
} from '@/lib/workspace/canvas/types';
import {
  CANVAS_WORLD,
  bringToFront,
  hitTestNodes,
  nextZ,
  sendToBack,
  snap,
} from '@/lib/workspace/canvas/geometry';
import { clampCamera, fitCameraToWorld, screenToWorld, zoomAt } from '@/lib/workspace/canvas/camera';
import {
  applyMarquee,
  applyMove,
  beginDrag,
  beginMarquee,
  beginPan,
  beginResize,
  commitOverride,
  nudgeDelta,
  nudgeNodes,
  type GestureDraft,
  type HandleKey,
  type Point,
} from '@/lib/workspace/canvas/gesture-controller';
import { canResize } from '@/lib/workspace/canvas/constraints';
import { getWidget, normalizeWidgetConfig, createDefaultConfig } from '@/lib/workspace/widgets';
import { Toolbar, type CanvasTool } from './canvas/Toolbar';
import { NodeSurface } from './canvas/NodeSurface';
import { ResizeOverlay } from './canvas/ResizeOverlay';
import { WidgetRenderer } from '../widgets/WidgetRenderer';

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

const EMPTY_DRAFT: Record<string, CanvasNode> = {};

export default forwardRef<FreeCanvasViewHandle, FreeCanvasViewProps>(function FreeCanvasView(
  { initialNodes, onCommit, editable = true, onSelectionChange }: FreeCanvasViewProps,
  ref,
) {
  const [nodes, setNodes] = useState<CanvasNode[]>(() => initialNodes);
  const [draft, setDraft] = useState<Record<string, CanvasNode>>(EMPTY_DRAFT);
  const [camera, setCamera] = useState<CanvasCamera>({ x: 300, y: 200, zoom: 1 });
  const [selection, setSelection] = useState<CanvasSelection>({ ids: [], mode: 'none' });
  const [tool, setTool] = useState<CanvasTool>('select');
  const [marquee, setMarquee] = useState<CanvasRect | null>(null);
  const [viewport, setViewport] = useState({ w: 800, h: 600 });
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const locale = useLocale();

  const stageRef = useRef<HTMLDivElement | null>(null);
  const gestureRef = useRef<GestureDraft | null>(null);
  const draftRef = useRef<Record<string, CanvasNode>>(EMPTY_DRAFT);
  const spaceRef = useRef(false);
  const commitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const onCommitRef = useRef(onCommit);
  onCommitRef.current = onCommit;
  const onSelectionChangeRef = useRef(onSelectionChange);
  onSelectionChangeRef.current = onSelectionChange;
  const cameraRef = useRef(camera);
  cameraRef.current = camera;
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const editableRef = useRef(editable);
  editableRef.current = editable;

  const updateSelection = useCallback((next: CanvasSelection) => {
    setSelection(next);
    onSelectionChangeRef.current?.(next.ids);
  }, []);

  const resetDraft = useCallback(() => {
    draftRef.current = EMPTY_DRAFT;
    setDraft(EMPTY_DRAFT);
  }, []);

  // Stage size tracking (coordinate transforms).
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

  // Space toggles the hand tool while held (window-level).
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

  /** Debounced single commit (pointer-up / keyboard path only). */
  const commitSoon = useCallback((next: CanvasNode[]) => {
    setNodes(next);
    if (commitTimerRef.current) clearTimeout(commitTimerRef.current);
    commitTimerRef.current = setTimeout(() => {
      onCommitRef.current(next);
      commitTimerRef.current = null;
    }, 120);
  }, []);

  /** Synchronous single commit (imperative host path). */
  const commitNow = useCallback((next: CanvasNode[]) => {
    setNodes(next);
    if (commitTimerRef.current) clearTimeout(commitTimerRef.current);
    onCommitRef.current(next);
    commitTimerRef.current = null;
  }, []);

  const worldAt = useCallback(
    (clientX: number, clientY: number): Point => {
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

  const screenPoint = useCallback((clientX: number, clientY: number): Point => {
    const stage = stageRef.current;
    if (!stage) return { x: 0, y: 0 };
    const rect = stage.getBoundingClientRect();
    return { x: clientX - rect.left, y: clientY - rect.top };
  }, []);

  // ── Stage: single pointer owner ──────────────────────────────────────
  const beginGesture = useCallback(
    (e: React.PointerEvent) => {
      if (!editableRef.current) return;
      if (e.button !== 0) return;
      // Cancel region: bodies and interactive node content never start a drag.
      const target = e.target as HTMLElement | null;
      if (target?.closest('[data-node-cancel]')) return;

      try {
        stageRef.current?.setPointerCapture(e.pointerId);
      } catch {
        /* capture unsupported — window-level move tracking still works */
      }
      const screenP = screenPoint(e.clientX, e.clientY);
      const world = worldAt(e.clientX, e.clientY);

      const cam = cameraRef.current;
      const usePan = tool === 'hand' || spaceRef.current;
      if (usePan) {
        gestureRef.current = beginPan(screenP, cam);
        return;
      }

      const hit = hitTestNodes(nodesRef.current, world.x, world.y);
      if (hit) {
        const ids = selectionRef.current.ids.includes(hit.id)
          ? selectionRef.current.ids
          : [hit.id];
        updateSelection({ ids, mode: 'single' });
        gestureRef.current = beginDrag(screenP, cam, hit.id, ids);
        return;
      }

      // Empty space → marquee select.
      updateSelection({ ids: [], mode: 'marquee' });
      setMarquee({ x: world.x, y: world.y, w: 0, h: 0 });
      gestureRef.current = beginMarquee(world, cam);
    },
    [tool, worldAt, screenPoint, updateSelection],
  );

  const beginResizeGesture = useCallback(
    (e: React.PointerEvent, nodeId: string, handle: HandleKey) => {
      if (!editableRef.current) return;
      e.stopPropagation();
      const node = nodesRef.current.find((item) => item.id === nodeId);
      if (!node || !canResize(node)) return;
      try {
        stageRef.current?.setPointerCapture(e.pointerId);
      } catch {
        /* ok */
      }
      const screenP = screenPoint(e.clientX, e.clientY);
      updateSelection({ ids: [nodeId], mode: 'single' });
      gestureRef.current = beginResize(screenP, cameraRef.current, node, handle);
    },
    [screenPoint, updateSelection],
  );

  const moveGesture = useCallback(
    (e: React.PointerEvent) => {
      const gesture = gestureRef.current;
      if (!gesture) return;
      const screen = screenPoint(e.clientX, e.clientY);
      if (gesture.mode === 'pan') {
        setCamera(
          clampCamera({
            ...gesture.startCamera,
            x: gesture.startCamera.x - (screen.x - gesture.startScreen.x) / gesture.startCamera.zoom,
            y: gesture.startCamera.y - (screen.y - gesture.startScreen.y) / gesture.startCamera.zoom,
          }),
        );
        return;
      }
      if (gesture.mode === 'marquee') {
        const world = worldAt(e.clientX, e.clientY);
        const result = applyMarquee(gesture, world, nodesRef.current);
        setMarquee(result.rect);
        updateSelection({ ids: result.ids, mode: 'marquee' });
        return;
      }
      // drag / resize → memory-only draft. No IPC, no DB write here (R-P11).
      // `screen` is the client-space point used to compute the world delta in
      // `applyMove` (it divides by the gesture-start zoom).
      const nextDraft = applyMove(gesture, screen, nodesRef.current);
      draftRef.current = nextDraft;
      setDraft(nextDraft);
    },
    [worldAt, screenPoint, updateSelection],
  );

  const endGesture = useCallback(
    (e?: React.PointerEvent) => {
      const gesture = gestureRef.current;
      gestureRef.current = null;
      setMarquee(null);
      if (e && stageRef.current) {
        try {
          stageRef.current.releasePointerCapture(e.pointerId);
        } catch {
          /* ok */
        }
      }
      if (gesture?.mode === 'drag' || gesture?.mode === 'resize') {
        // Single commit on the valid pointer-up. Empty draft = nothing moved.
        const overrides = draftRef.current;
        if (Object.keys(overrides).length === 0) {
          resetDraft();
          return;
        }
        const next = commitOverride(nodesRef.current, overrides);
        resetDraft();
        setNodes(next);
        onCommitRef.current(next);
      }
    },
    [resetDraft],
  );

  const cancelGesture = useCallback(() => {
    const gesture = gestureRef.current;
    gestureRef.current = null;
    setMarquee(null);
    if (gesture?.mode === 'drag' || gesture?.mode === 'resize') {
      resetDraft(); // rollback: canonical nodes stay untouched.
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
        setCamera((c) => clampCamera(zoomAt(c, { x: viewport.w, y: viewport.h }, screen, factor)));
      } else {
        setCamera((c) =>
          clampCamera({ ...c, x: c.x + e.deltaX / c.zoom, y: c.y + e.deltaY / c.zoom }),
        );
      }
    },
    [viewport],
  );

  // ── Node ops ──────────────────────────────────────────────────────────
  const addNode = useCallback(
    (kind: CanvasNodeKind) => {
      const count = nodesRef.current.length;
      const node = createCanvasNode({
        id: `node-${Date.now().toString(36)}-${count}`,
        kind,
        label: nodeKindLabel(kind, count, locale),
        x: snap(60 + (count % 5) * 40),
        y: snap(60 + (count % 4) * 40),
        z: nextZ(nodesRef.current),
        accent: kind === 'note' ? '--primary-soft' : kind === 'frame' ? '--surface-hover' : '--surface',
      });
      commitSoon([...nodesRef.current, node]);
      updateSelection({ ids: [node.id], mode: 'single' });
    },
    [commitSoon, updateSelection, locale],
  );

  const handleAddWidget = useCallback(
    (widgetType: string) => {
      const count = nodesRef.current.length;
      const node = createCanvasNode({
        id: `widget-${Date.now().toString(36)}-${count}`,
        kind: 'widget',
        widgetType,
        label: widgetType,
        x: snap(60 + (count % 5) * 40),
        y: snap(60 + (count % 4) * 40),
        z: nextZ(nodesRef.current),
      });
      commitSoon([...nodesRef.current, node]);
      updateSelection({ ids: [node.id], mode: 'single' });
    },
    [commitSoon, updateSelection],
  );

  const groupSelection = useCallback(() => {
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
  }, [commitSoon, updateSelection, locale]);

  const ungroupSelection = useCallback(() => {
    const group = nodesRef.current.find(
      (node) => node.kind === 'group' && selectionRef.current.ids.includes(node.id),
    );
    if (!group) return;
    commitSoon(nodesRef.current.filter((node) => node.id !== group.id));
    updateSelection({ ids: group.members ?? [], mode: 'single' });
  }, [commitSoon, updateSelection]);

  const deleteSelection = useCallback(() => {
    const ids = selectionRef.current.ids;
    if (ids.length === 0) return;
    const remaining = nodesRef.current.filter((node) => !ids.includes(node.id));
    commitSoon(remaining);
    updateSelection({ ids: [], mode: 'none' });
  }, [commitSoon, updateSelection]);

  // ── Keyboard: nudges, group / z-order, focus-scoped Delete/Escape ─────
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      // Focus protection (WS-04): typing in an input never triggers a global
      // Delete / Backspace / Escape.
      if (target?.matches('input,textarea,[contenteditable=true]')) return;
      const ids = selectionRef.current.ids;
      if (ids.length === 0) return;
      if (e.key === 'Delete' || e.key === 'Backspace') {
        e.preventDefault();
        deleteSelection();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        updateSelection({ ids: [], mode: 'none' });
      } else if (e.key.startsWith('Arrow')) {
        e.preventDefault();
        const delta = nudgeDelta(e.key, e.shiftKey);
        if (!delta) return;
        // Keyboard stop: exactly one commit (arrow = fully-snapped + persisted).
        const next = nudgeNodes(nodesRef.current, ids, delta.x, delta.y);
        setNodes(next);
        onCommitRef.current(next);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'g') {
        e.preventDefault();
        if (e.shiftKey) ungroupSelection();
        else groupSelection();
      } else if ((e.metaKey || e.ctrlKey) && e.key === ']') {
        e.preventDefault();
        commitSoon(bringToFront(nodesRef.current, ids));
      } else if ((e.metaKey || e.ctrlKey) && e.key === '[') {
        e.preventDefault();
        commitSoon(sendToBack(nodesRef.current, ids));
      }
    },
    [commitSoon, deleteSelection, groupSelection, ungroupSelection, updateSelection],
  );

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  useImperativeHandle(
    ref,
    () => ({
      deleteSelection,
      groupSelection,
      ungroupSelection,
      bringToFront: () => commitNow(bringToFront(nodesRef.current, selectionRef.current.ids)),
      sendToBack: () => commitNow(sendToBack(nodesRef.current, selectionRef.current.ids)),
    }),
    [commitNow, deleteSelection, groupSelection, ungroupSelection],
  );

  const zoomIn = useCallback(
    () =>
      setCamera((c) =>
        clampCamera(zoomAt(c, { x: viewport.w, y: viewport.h }, { x: viewport.w / 2, y: viewport.h / 2 }, 1 + 0.12)),
      ),
    [viewport],
  );
  const zoomOut = useCallback(
    () =>
      setCamera((c) =>
        clampCamera(zoomAt(c, { x: viewport.w, y: viewport.h }, { x: viewport.w / 2, y: viewport.h / 2 }, 1 - 0.12)),
      ),
    [viewport],
  );
  const zoomFit = useCallback(
    () => setCamera(fitCameraToWorld(CANVAS_WORLD.w, CANVAS_WORLD.h, { x: viewport.w, y: viewport.h })),
    [viewport],
  );

  // ── Render ────────────────────────────────────────────────────────────
  const nodesToRender = useMemo(
    () =>
      nodes.map((node) => {
        const override = draft[node.id];
        return override ? { ...node, ...override } : node;
      }),
    [nodes, draft],
  );

  const selectedNode = useMemo(
    () => nodesToRender.find((node) => node.id === selection.ids[0] && canResize(node)),
    [nodesToRender, selection.ids],
  );

  const worldScreenX = viewport.w / 2 - camera.x * camera.zoom;
  const worldScreenY = viewport.h / 2 - camera.y * camera.zoom;

  const activeTool = spaceRef.current ? 'hand' : tool;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      {editable && (
        <Toolbar
          tool={activeTool}
          zoom={camera.zoom}
          selectionCount={selection.ids.length}
          addMenuOpen={addMenuOpen}
          onToolChange={setTool}
          onAddNote={() => addNode('note')}
          onAddFrame={() => addNode('frame')}
          onGroup={groupSelection}
          onUngroup={ungroupSelection}
          onBringToFront={() => commitSoon(bringToFront(nodesRef.current, selection.ids))}
          onSendToBack={() => commitSoon(sendToBack(nodesRef.current, selection.ids))}
          onDelete={deleteSelection}
          onZoomIn={zoomIn}
          onZoomOut={zoomOut}
          onFit={zoomFit}
          onAddMenuOpenChange={setAddMenuOpen}
          onAddWidget={handleAddWidget}
        />
      )}

      <div
        ref={stageRef}
        className="relative min-h-0 flex-1 overflow-hidden bg-[var(--surface-subtle)]"
        onPointerDown={beginGesture}
        onPointerMove={moveGesture}
        onPointerUp={endGesture}
        onPointerCancel={cancelGesture}
        onWheel={handleWheel}
        style={{ cursor: activeTool === 'hand' ? 'grab' : 'default', touchAction: 'none' }}
        data-tool={activeTool}
        data-testid="canvas-stage"
      >
        <div
          className="absolute origin-top-left"
          style={{
            left: worldScreenX,
            top: worldScreenY,
            width: CANVAS_WORLD.w,
            height: CANVAS_WORLD.h,
            transform: `scale(${camera.zoom})`,
            backgroundImage: editable
              ? 'repeating-linear-gradient(0deg, var(--border-subtle) 0 1px, transparent 1px 16px), repeating-linear-gradient(90deg, var(--border-subtle) 0 1px, transparent 1px 16px)'
              : 'none',
          }}
        >
          {nodesToRender.map((node) => (
            <NodeSurface
              key={node.id}
              node={node}
              selected={selection.ids.includes(node.id)}
              content={nodeContent(node)}
            />
          ))}
        </div>

        {/* Selected node resize overlay: OUTSIDE the scaled world so handle
            hit areas stay constant screen px (WS-03 / R-U19). */}
        {editable && selectedNode && (
          <ResizeOverlay
            screenRect={{
              x: selectedNode.x * camera.zoom + worldScreenX,
              y: selectedNode.y * camera.zoom + worldScreenY,
              w: selectedNode.w * camera.zoom,
              h: selectedNode.h * camera.zoom,
            }}
            onResizeStart={(handle, event) => beginResizeGesture(event, selectedNode.id, handle)}
          />
        )}

        {marquee && (
          <div
            className="pointer-events-none absolute border border-[var(--primary)] bg-[var(--primary-soft)]/25"
            style={{
              left: worldScreenX + marquee.x * camera.zoom,
              top: worldScreenY + marquee.y * camera.zoom,
              width: marquee.w * camera.zoom,
              height: marquee.h * camera.zoom,
            }}
          />
        )}
      </div>
    </div>
  );
});

/** Widget-body content for a widget node (Web-registered WidgetRenderer). */
function nodeContent(node: CanvasNode): React.ReactNode {
  if (!node.widgetType) return undefined;
  const widgetDef = getWidget(node.widgetType);
  if (!widgetDef) return undefined;
  const config = normalizeWidgetConfig(widgetDef, {
    ...createDefaultConfig(widgetDef),
    ...(node.widgetConfig ?? {}),
    order: 0,
  });
  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-xl">
      <WidgetRenderer instance={{ def: widgetDef, config }} editing={false} />
    </div>
  );
}

/** Plain node label text (i18n; frame / note use their own fixed label). */
function nodeKindLabel(kind: CanvasNodeKind, count: number, locale: string): string {
  if (kind === 'frame') return t(locale, 'workspace.canvasNodeFrameLabel');
  if (kind === 'note') return t(locale, 'workspace.canvasNodeNoteLabel');
  return t(locale, 'workspace.canvasNodeLabel', { n: count + 1 });
}