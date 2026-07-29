'use client';

import {
  ArchiveRestore,
  Bot,
  Braces,
  CheckCircle2,
  CircleStop,
  Flag,
  GitBranch,
  Layers3,
  MessageSquareText,
  Network,
  RadioTower,
  RotateCcw,
  ShieldCheck,
  Wrench,
  ZoomIn,
  ZoomOut,
} from 'lucide-react';
import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import {
  NODE_HEIGHT,
  NODE_WIDTH,
  edgeLabelKey,
  enabledHookCount,
  layoutStages,
  sortStages,
  stageEvidenceCode,
  stagePresentation,
  visibleEdges,
  type CanvasEdge,
  type CanvasMode,
  type CanvasNodeDetail,
  type CanvasRunSnapshot,
  type CanvasStage,
  type CanvasTraceEntry,
  type StageEvidenceCode,
  type StageIconId,
} from './nativeExecutionCanvasModel';

const EVIDENCE_KEYS: Record<StageEvidenceCode, string> = {
  loaded: 'settings.engineCanvasEvidenceLoaded',
  configured: 'settings.engineCanvasEvidenceConfigured',
  attention: 'settings.engineCanvasEvidenceAttention',
  choose_run: 'settings.engineCanvasEvidenceChooseRun',
  insufficient: 'settings.engineCanvasEvidenceInsufficient',
  no_evidence: 'settings.engineCanvasEvidenceNone',
  running: 'settings.engineCanvasEvidenceRunning',
  hook_evidence: 'settings.engineCanvasEvidenceHook',
  snapshot_recorded: 'settings.engineCanvasEvidenceSnapshot',
  failed: 'settings.engineCanvasEvidenceFailed',
};

const EVIDENCE_CLASSES: Record<StageEvidenceCode, string> = {
  loaded: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  configured: 'bg-[var(--info-soft)] text-[var(--info)]',
  attention: 'bg-[var(--warning-soft)] text-[var(--warning)]',
  choose_run: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  insufficient: 'bg-[var(--warning-soft)] text-[var(--warning)]',
  no_evidence: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  running: 'bg-[var(--info-soft)] text-[var(--info)]',
  hook_evidence: 'bg-[var(--success-soft)] text-[var(--success)]',
  snapshot_recorded: 'bg-[var(--info-soft)] text-[var(--info)]',
  failed: 'bg-[var(--danger-soft)] text-[var(--danger)]',
};

function StageIcon({ id }: { id: StageIconId }) {
  const props = { size: 17, 'aria-hidden': true as const };
  switch (id) {
    case 'session': return <MessageSquareText {...props} />;
    case 'context': return <Layers3 {...props} />;
    case 'provider': return <Bot {...props} />;
    case 'tool_gate': return <Braces {...props} />;
    case 'permission': return <ShieldCheck {...props} />;
    case 'tool_execute': return <Wrench {...props} />;
    case 'subagent': return <Network {...props} />;
    case 'compact': return <ArchiveRestore {...props} />;
    case 'stop': return <CircleStop {...props} />;
    case 'terminal': return <Flag {...props} />;
    case 'cross_stage': return <RadioTower {...props} />;
    default: return <GitBranch {...props} />;
  }
}

function edgePath(
  edge: CanvasEdge,
  positions: Map<string, { x: number; y: number }>,
): { d: string; labelX: number; labelY: number } | null {
  const from = positions.get(edge.from);
  const to = positions.get(edge.to);
  if (!from || !to) return null;

  if (edge.kind === 'loop') {
    const x1 = from.x + NODE_WIDTH / 2;
    const x2 = to.x + NODE_WIDTH / 2;
    const y = Math.min(from.y, to.y) - 40;
    return {
      d: `M ${x1} ${from.y} C ${x1} ${y}, ${x2} ${y}, ${x2} ${to.y}`,
      labelX: (x1 + x2) / 2,
      labelY: y - 6,
    };
  }

  const horizontal = Math.abs(from.y - to.y) < 24;
  if (horizontal) {
    const x1 = from.x + NODE_WIDTH;
    const y1 = from.y + NODE_HEIGHT / 2;
    const x2 = to.x;
    const y2 = to.y + NODE_HEIGHT / 2;
    const bend = Math.max(28, Math.abs(x2 - x1) / 2);
    return {
      d: `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`,
      labelX: (x1 + x2) / 2,
      labelY: y1 - 9,
    };
  }

  const x1 = from.x + NODE_WIDTH / 2;
  const y1 = from.y + NODE_HEIGHT;
  const x2 = to.x + NODE_WIDTH / 2;
  const y2 = to.y;
  const bend = Math.max(36, Math.abs(y2 - y1) / 2);
  return {
    d: `M ${x1} ${y1} C ${x1} ${y1 + bend}, ${x2} ${y2 - bend}, ${x2} ${y2}`,
    labelX: (x1 + x2) / 2,
    labelY: (y1 + y2) / 2 - 7,
  };
}

type GraphProps = {
  locale: Locale;
  stages: CanvasStage[];
  edges: CanvasEdge[];
  promptBlockCount: number;
  mode: CanvasMode;
  selectedStageId: string | null;
  selectedRunId?: string;
  traceEntries?: CanvasTraceEntry[];
  runSnapshot?: CanvasRunSnapshot | null;
  nodeDetails?: Record<string, CanvasNodeDetail>;
  onSelectStage: (stageId: string) => void;
};

function StageNode({
  locale,
  stage,
  promptBlockCount,
  mode,
  selected,
  selectedRunId,
  traceEntries,
  runSnapshot,
  detail,
  onSelect,
  className = '',
}: {
  locale: Locale;
  stage: CanvasStage;
  promptBlockCount: number;
  mode: CanvasMode;
  selected: boolean;
  selectedRunId?: string;
  traceEntries?: CanvasTraceEntry[];
  runSnapshot?: CanvasRunSnapshot | null;
  detail?: CanvasNodeDetail;
  onSelect: () => void;
  className?: string;
}) {
  const presentation = stagePresentation(stage.id, locale);
  const evidence = stageEvidenceCode({
    mode,
    stage,
    promptBlockCount,
    selectedRunId,
    traceEntries,
    runSnapshot,
  });
  const hooks = enabledHookCount(stage);
  const promptCount = stage.id === 'context' ? promptBlockCount : 0;
  const detailSummary = detail?.runResults?.length
    ? t(locale, 'settings.engineCanvasNodeResultCount', { count: detail.runResults.length })
    : detail?.tools?.length
      ? t(locale, 'settings.engineCanvasNodeToolCount', { count: detail.tools.length })
      : detail?.subagents?.length
        ? t(locale, 'settings.engineCanvasNodeSubagentCount', { count: detail.subagents.length })
        : detail?.prompts?.length
          ? t(locale, 'settings.engineCanvasPromptCount', { count: detail.prompts.length })
          : null;

  return (
    <button
      type="button"
      className={`interactive-node group flex h-full w-full flex-col gap-2 rounded-lg border p-3 text-left shadow-sm transition-colors ${
        selected
          ? 'border-[var(--primary)] bg-[var(--surface)] ring-2 ring-[var(--primary-soft)]'
          : 'border-[var(--border)] bg-[var(--surface)] hover:border-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
      } ${className}`}
      aria-pressed={selected}
      aria-label={`${stage.order != null ? stage.order + 1 : ''} ${presentation.title}. ${presentation.description}`}
      onClick={onSelect}
    >
      <span className="flex w-full items-center justify-between gap-2">
        <span className="inline-flex min-w-0 items-center gap-2 text-[var(--text)]">
          <StageIcon id={presentation.icon} />
          <span className="truncate text-sm font-semibold">{presentation.title}</span>
        </span>
        <span className="shrink-0 font-mono text-[10px] text-[var(--text-disabled)]">
          {stage.order != null ? `#${String(stage.order + 1).padStart(2, '0')}` : '—'}
        </span>
      </span>

      <span className="line-clamp-2 min-h-8 text-xs leading-4 text-[var(--text-secondary)]">
        {presentation.description}
      </span>

      <span className="flex w-full items-center justify-between gap-2">
        <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${EVIDENCE_CLASSES[evidence]}`}>
          {t(locale, EVIDENCE_KEYS[evidence])}
        </span>
        <span className="truncate text-[10px] text-[var(--text-disabled)]">
          {detailSummary ?? (hooks > 0
            ? t(locale, 'settings.engineCanvasEnabledHooks', { count: hooks })
            : promptCount > 0
              ? t(locale, 'settings.engineCanvasPromptCount', { count: promptCount })
              : presentation.technicalName)}
        </span>
      </span>
    </button>
  );
}

export function NativeExecutionGraph(props: GraphProps) {
  const {
    locale,
    stages,
    edges,
    promptBlockCount,
    mode,
    selectedStageId,
    selectedRunId,
    traceEntries,
    runSnapshot,
    nodeDetails,
    onSelectStage,
  } = props;
  const ordered = useMemo(() => sortStages(stages), [stages]);
  const layout = useMemo(() => layoutStages(ordered, locale), [locale, ordered]);
  const renderedEdges = useMemo(() => visibleEdges(ordered, edges), [edges, ordered]);
  const viewportRef = useRef<HTMLDivElement>(null);
  const [zoom, setZoom] = useState(0.9);
  const [pan, setPan] = useState({ x: 12, y: 12 });
  const dragRef = useRef<{ x: number; y: number; panX: number; panY: number } | null>(null);
  const markerId = useId().replace(/:/g, '');

  const fitView = useCallback(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const nextZoom = Math.min(
      1,
      Math.max(0.72, Math.min((viewport.clientWidth - 32) / layout.width, (viewport.clientHeight - 32) / layout.height)),
    );
    setZoom(nextZoom);
    setPan({
      x: Math.max(16, (viewport.clientWidth - layout.width * nextZoom) / 2),
      y: Math.max(16, (viewport.clientHeight - layout.height * nextZoom) / 2),
    });
  }, [layout.height, layout.width]);

  useEffect(() => {
    fitView();
    const viewport = viewportRef.current;
    if (!viewport || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(fitView);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, [fitView]);

  return (
    <>
      <div
        ref={viewportRef}
        className="relative hidden min-h-[620px] overflow-hidden bg-[var(--background)] lg:block"
        role="region"
        aria-label={t(locale, 'settings.engineCanvasGraphLabel')}
        onPointerDown={(event) => {
          if ((event.target as HTMLElement).closest('button')) return;
          dragRef.current = { x: event.clientX, y: event.clientY, panX: pan.x, panY: pan.y };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const drag = dragRef.current;
          if (!drag) return;
          setPan({ x: drag.panX + event.clientX - drag.x, y: drag.panY + event.clientY - drag.y });
        }}
        onPointerUp={(event) => {
          dragRef.current = null;
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            event.currentTarget.releasePointerCapture(event.pointerId);
          }
        }}
      >
        <div className="absolute right-3 top-3 z-20 flex items-center gap-1 rounded-md border border-[var(--border)] bg-[var(--surface)] p-1 shadow-sm">
          <button
            type="button"
            className="btn btn-ghost h-7 w-7 p-0"
            aria-label={t(locale, 'settings.engineCanvasZoomIn')}
            onClick={() => setZoom((value) => Math.min(1.25, value + 0.1))}
          ><ZoomIn size={14} /></button>
          <span className="w-9 text-center text-[10px] text-[var(--text-secondary)]">{Math.round(zoom * 100)}%</span>
          <button
            type="button"
            className="btn btn-ghost h-7 w-7 p-0"
            aria-label={t(locale, 'settings.engineCanvasZoomOut')}
            onClick={() => setZoom((value) => Math.max(0.6, value - 0.1))}
          ><ZoomOut size={14} /></button>
          <button
            type="button"
            className="btn btn-ghost h-7 w-7 p-0"
            aria-label={t(locale, 'settings.engineCanvasFit')}
            onClick={fitView}
          ><RotateCcw size={14} /></button>
        </div>

        <div
          className="absolute left-0 top-0 origin-top-left transition-transform duration-100"
          style={{
            width: layout.width,
            height: layout.height,
            transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
          }}
        >
          {layout.groups.map((group) => (
            <div
              key={group.id}
              className="absolute flex items-center gap-2 text-[11px] font-medium text-[var(--text-secondary)]"
              style={{ left: group.x, top: group.y }}
            >
              <span className="h-px w-5 bg-[var(--border)]" />
              {group.label}
            </div>
          ))}

          <svg className="pointer-events-none absolute inset-0" width={layout.width} height={layout.height} aria-hidden="true">
            <defs>
              <marker id={`${markerId}-arrow`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto">
                <path d="M 0 1 L 10 5 L 0 9 z" fill="var(--text-disabled)" />
              </marker>
              <marker id={`${markerId}-active-arrow`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto">
                <path d="M 0 1 L 10 5 L 0 9 z" fill="var(--text)" />
              </marker>
            </defs>
            {renderedEdges.map((edge) => {
              const path = edgePath(edge, layout.positions);
              if (!path) return null;
              const active = selectedStageId === edge.from || selectedStageId === edge.to;
              const labelKey = edgeLabelKey(edge);
              return (
                <g key={`${edge.from}:${edge.to}:${edge.kind ?? 'flow'}`}>
                  <path
                    d={path.d}
                    fill="none"
                    stroke={active ? 'var(--text)' : edge.kind === 'signal' ? 'var(--info)' : 'var(--text-disabled)'}
                    strokeWidth={active ? 2 : 1.25}
                    strokeDasharray={edge.kind === 'signal' ? '5 5' : undefined}
                    markerEnd={`url(#${active ? `${markerId}-active-arrow` : `${markerId}-arrow`})`}
                  />
                  {labelKey ? (
                    <text
                      x={path.labelX}
                      y={path.labelY}
                      textAnchor="middle"
                      fill="var(--text-secondary)"
                      fontSize="10"
                    >
                      {t(locale, labelKey)}
                    </text>
                  ) : null}
                </g>
              );
            })}
          </svg>

          {ordered.map((stage) => {
            const position = layout.positions.get(stage.id);
            if (!position) return null;
            return (
              <div
                key={stage.id}
                className="absolute"
                style={{ left: position.x, top: position.y, width: NODE_WIDTH, height: NODE_HEIGHT }}
              >
                <StageNode
                  locale={locale}
                  stage={stage}
                  promptBlockCount={promptBlockCount}
                  mode={mode}
                  selected={selectedStageId === stage.id}
                  selectedRunId={selectedRunId}
                  traceEntries={traceEntries}
                  runSnapshot={runSnapshot}
                  detail={nodeDetails?.[stage.id]}
                  onSelect={() => onSelectStage(stage.id)}
                />
              </div>
            );
          })}
        </div>
      </div>

      <div className="space-y-3 bg-[var(--background)] p-3 lg:hidden" aria-label={t(locale, 'settings.engineCanvasMobileLabel')}>
        {ordered.map((stage, index) => {
          const presentation = stagePresentation(stage.id, locale);
          const previousStage = ordered[index - 1];
          const previous = previousStage ? stagePresentation(previousStage.id, locale) : null;
          const showGroup = previous?.group !== presentation.group;
          return (
            <div key={stage.id}>
              {showGroup ? (
                <div className="mb-2 mt-3 flex items-center gap-2 text-xs font-medium text-[var(--text-secondary)] first:mt-0">
                  <span className="h-px w-5 bg-[var(--border)]" />{presentation.groupLabel}
                </div>
              ) : null}
              <div className="relative pl-5">
                {index < ordered.length - 1 ? <span className="absolute bottom-[-14px] left-[5px] top-5 w-px bg-[var(--border)]" /> : null}
                <span className="absolute left-0 top-5 h-2.5 w-2.5 rounded-full border-2 border-[var(--surface)] bg-[var(--text-secondary)]" />
                <StageNode
                  {...props}
                  stage={stage}
                  selected={selectedStageId === stage.id}
                  detail={nodeDetails?.[stage.id]}
                  onSelect={() => onSelectStage(stage.id)}
                  className="w-full"
                />
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
}
