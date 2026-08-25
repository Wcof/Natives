'use client';

/**
 * Toolbar (WS-03) — pan / zoom / align / viewport controls for the Free
 * Canvas. Rendered above the Stage by the view container. Uses semantic tokens
 * only (R-U1/R-E4), SVG icons (R-U6), i18n text (R-I1), and keyboard-visible
 * labels.
 */

import { useRef } from 'react';
import { useLocale, t } from '@/i18n';
import {
  BringToFront,
  Frame,
  Hand,
  LayoutGrid,
  Layers,
  Maximize,
  Minus,
  MousePointer2,
  Plus,
  SendToBack,
  StickyNote,
  Trash2,
  Ungroup,
} from 'lucide-react';
import { AddWidgetMenu } from '../../widgets/AddWidgetMenu';

export type CanvasTool = 'select' | 'hand';

export interface ToolbarProps {
  tool: CanvasTool;
  zoom: number;
  selectionCount: number;
  addMenuOpen: boolean;
  onToolChange: (tool: CanvasTool) => void;
  onAddNote: () => void;
  onAddFrame: () => void;
  onGroup: () => void;
  onUngroup: () => void;
  onBringToFront: () => void;
  onSendToBack: () => void;
  onDelete: () => void;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFit: () => void;
  onAddMenuOpenChange: (open: boolean) => void;
  onAddWidget: (widgetType: string) => void;
}

export function Toolbar(props: ToolbarProps) {
  const locale = useLocale();
  const addWidgetButtonRef = useRef<HTMLButtonElement | null>(null);
  const {
    tool,
    zoom,
    selectionCount,
    addMenuOpen,
    onToolChange,
    onAddNote,
    onAddFrame,
    onGroup,
    onUngroup,
    onBringToFront,
    onSendToBack,
    onDelete,
    onZoomIn,
    onZoomOut,
    onFit,
    onAddMenuOpenChange,
    onAddWidget,
  } = props;

  return (
    <div
      className="flex h-10 shrink-0 items-center gap-1 border-b border-[var(--border-subtle)] px-2"
      role="toolbar"
      aria-label={t(locale, 'workspace.canvasToolbar')}
      data-testid="canvas-toolbar"
    >
      <ToolButton active={tool === 'select'} onClick={() => onToolChange('select')} title={t(locale, 'workspace.canvasSelect')} icon={<MousePointer2 size={14} />} />
      <ToolButton active={tool === 'hand'} onClick={() => onToolChange('hand')} title={t(locale, 'workspace.canvasPan')} icon={<Hand size={14} />} />
      <span className="mx-1 h-4 w-px bg-[var(--border-subtle)]" />
      <div className="relative">
        <ToolButton
          active={addMenuOpen}
          refTo={addWidgetButtonRef}
          onClick={() => onAddMenuOpenChange(!addMenuOpen)}
          title={t(locale, 'workspace.addCard')}
          icon={<LayoutGrid size={13} />}
          aria-expanded={addMenuOpen}
          aria-haspopup="menu"
        />
        <AddWidgetMenu
          open={addMenuOpen}
          onOpenChange={onAddMenuOpenChange}
          triggerRef={addWidgetButtonRef}
          onSelect={onAddWidget}
        />
      </div>
      <ToolButton onClick={onAddNote} title={t(locale, 'workspace.canvasAddNote')} icon={<StickyNote size={14} />} />
      <ToolButton onClick={onAddFrame} title={t(locale, 'workspace.canvasAddFrame')} icon={<Frame size={14} />} />
      {selectionCount > 0 && (
        <>
          <span className="mx-1 h-4 w-px bg-[var(--border-subtle)]" />
          <ToolButton onClick={onGroup} title={t(locale, 'workspace.canvasGroup')} icon={<Layers size={14} />} />
          <ToolButton onClick={onUngroup} title={t(locale, 'workspace.canvasUngroup')} icon={<Ungroup size={14} />} />
          <ToolButton onClick={onBringToFront} title={t(locale, 'workspace.canvasFront')} icon={<BringToFront size={14} />} />
          <ToolButton onClick={onSendToBack} title={t(locale, 'workspace.canvasBack')} icon={<SendToBack size={14} />} />
          <ToolButton onClick={onDelete} title={t(locale, 'workspace.canvasDelete')} danger icon={<Trash2 size={14} />} />
        </>
      )}
      <div className="ml-auto flex items-center gap-0.5">
        <ToolButton onClick={onZoomOut} title={t(locale, 'workspace.canvasZoomOut')} icon={<Minus size={14} />} />
        <span className="min-w-9 text-center text-[0.625rem] tabular-nums text-[var(--text-disabled)]" data-testid="canvas-zoom-level">
          {Math.round(zoom * 100)}%
        </span>
        <ToolButton onClick={onZoomIn} title={t(locale, 'workspace.canvasZoomIn')} icon={<Plus size={14} />} />
        <ToolButton onClick={onFit} title={t(locale, 'workspace.canvasFit')} icon={<Maximize size={14} />} />
      </div>
    </div>
  );
}

function ToolButton({
  onClick,
  title,
  icon,
  active,
  danger,
  refTo,
  ...rest
}: {
  onClick: () => void;
  title: string;
  icon: React.ReactNode;
  active?: boolean;
  danger?: boolean;
  refTo?: React.RefObject<HTMLButtonElement | null>;
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      ref={refTo}
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      {...rest}
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