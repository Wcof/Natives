'use client';

import React, { useState } from 'react';
import { SPACING, BORDER_RADIUS, FONT_SIZE } from '@/lib/design-tokens';
import { ChevronDown, ChevronRight } from 'lucide-react';

interface KanbanCardProps {
  /** Card icon (lucide component) */
  icon: React.ReactNode;
  /** Card title */
  title: string;
  /** Summary value — shown in collapsed state */
  summary: React.ReactNode;
  /** Badge/label shown next to summary (e.g. "12 skills") */
  badge?: string;
  /** Accent color for the card */
  accentColor?: string;
  /** Whether the card is loading */
  isLoading?: boolean;
  /** Fallback text when no data */
  emptyText?: string;
  /** Expanded content (the full component) */
  children: React.ReactNode;
  /** Default collapsed state */
  defaultExpanded?: boolean;
  /** Controlled expanded state */
  expanded?: boolean;
  /** Controlled toggle handler */
  onToggle?: () => void;
}

/**
 * KanbanCard — Collapsible dashboard card.
 *
 * Collapsed: compact row showing icon + title + key metric (fits many on screen).
 * Expanded: reveals full children content with smooth animation.
 */
export default function KanbanCard({
  icon,
  title,
  summary,
  badge,
  accentColor = 'var(--vibe-accent-color)',
  isLoading,
  emptyText,
  children,
  defaultExpanded = false,
  expanded: expandedProp,
  onToggle,
}: KanbanCardProps) {
  const [localExpanded, setLocalExpanded] = useState(defaultExpanded);
  const isControlled = expandedProp !== undefined;
  const expanded = isControlled ? expandedProp : localExpanded;

  const [hovered, setHovered] = useState(false);
  const [active, setActive] = useState(false);

  const handleToggle = () => {
    if (isControlled && onToggle) {
      onToggle();
    } else {
      setLocalExpanded(!localExpanded);
    }
  };

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => { setHovered(false); setActive(false); }}
      style={{
        position: 'relative',
        borderRadius: BORDER_RADIUS.lg,
        border: `0.0625rem solid ${hovered ? accentColor : 'var(--vibe-content-border)'}`,
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
        overflow: 'hidden',
        transform: active ? 'scale(0.995) translateY(-1px)' : hovered ? 'translateY(-1px)' : 'none',
        boxShadow: hovered ? `0 12px 36px ${accentColor}12, 0 0 0 1px ${accentColor}25` : 'var(--vibe-content-shadow)',
        transition: 'border-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s cubic-bezier(0.16, 1, 0.3, 1)',
      }}
    >
      {/* Top accent indicator bar */}
      <div
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          right: 0,
          height: 3,
          background: accentColor,
          opacity: expanded ? 1 : hovered ? 0.6 : 0,
          transition: 'opacity 0.2s ease',
          zIndex: 10,
        }}
      />

      {/* Collapsed header — always visible, click to toggle */}
      <div
        onClick={handleToggle}
        onMouseDown={() => setActive(true)}
        onMouseUp={() => setActive(false)}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { handleToggle(); e.preventDefault(); } }}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: SPACING.sm,
          padding: `${SPACING.sm}px ${SPACING.md}px`,
          cursor: 'pointer',
          userSelect: 'none',
          minHeight: 48,
          borderBottom: expanded ? '1px solid var(--vibe-content-border)' : '0px solid transparent',
          transition: 'background 0.12s, border-color 0.2s',
          paddingTop: SPACING.sm + 2, // offset for top accent bar
        }}
        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)'; }}
        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
      >
        {/* Icon */}
        <div
          style={{
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            width: 32, height: 32, borderRadius: BORDER_RADIUS.md,
            background: `${accentColor}18`, color: accentColor, flexShrink: 0,
            transform: hovered ? 'scale(1.08) rotate(6deg)' : 'none',
            transition: 'transform 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275)',
          }}
        >
          {icon}
        </div>

        {/* Title */}
        <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)', flexShrink: 0 }}>
          {title}
        </span>

        {/* Summary value */}
        <div style={{ flex: 1, textAlign: 'right', minWidth: 0, overflow: 'hidden' }}>
          {isLoading ? (
            <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)' }}>…</span>
          ) : (
            <span style={{ fontSize: FONT_SIZE.md, fontWeight: 700, color: 'var(--vibe-brand-text)', fontFamily: 'var(--font-mono)' }}>
              {summary}
            </span>
          )}
        </div>

        {/* Badge */}
        {badge && !isLoading && (
          <span style={{
            fontSize: FONT_SIZE.xs, color: 'var(--text-dim)', background: 'var(--vibe-btn-bg)',
            padding: `1px ${SPACING.xs}px`, borderRadius: 999, whiteSpace: 'nowrap',
          }}>
            {badge}
          </span>
        )}

        {/* Expand indicator */}
        <div style={{ color: 'var(--text-faint)', flexShrink: 0, transition: 'transform 0.2s' }}>
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </div>
      </div>

      {/* Expanded content — animated reveal */}
      <div
        style={{
          display: 'grid',
          gridTemplateRows: expanded ? '1fr' : '0fr',
          transition: 'grid-template-rows 0.25s ease',
        }}
      >
        <div style={{ overflow: 'hidden' }}>
          <div style={{ padding: `0 ${SPACING.md}px ${expanded ? SPACING.md : 0}px` }}>
            {children}
          </div>
        </div>
      </div>
    </div>
  );
}
