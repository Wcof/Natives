'use client';

/**
 * AddWidgetMenu (Slice 14) — the "Add widget" menu for the grid view.
 *
 *  - The menu is driven ENTIRELY by the widget registry (getRegisteredTypes +
 *    getWidget): no hardcoded widget list. Each entry reuses the registered
 *    definition's localized titleKey (falls back to the type id).
 *  - Keyboard-dismissable (Escape) and click-outside close; focus returns to
 *    the trigger. Empty-state-safe (the registry is always non-empty for
 *    built-ins, but the UI degrades gracefully if it ever is).
 *  - Selecting a type invokes onSelect(widgetType) and closes the menu.
 */

import { useEffect, useMemo, useRef } from 'react';
import { Plus } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { getRegisteredTypes, getWidget } from '@/lib/workspace/widgets';

export interface AddWidgetMenuProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The trigger button ref (focus target when the menu closes). */
  triggerRef: React.RefObject<HTMLElement | null>;
  /** Called with the selected registry type id; the menu closes after. */
  onSelect: (widgetType: string) => void;
}

export function AddWidgetMenu({ open, onOpenChange, triggerRef, onSelect }: AddWidgetMenuProps) {
  const locale = useLocale();
  const menuRef = useRef<HTMLDivElement | null>(null);

  // Registry-driven entries (no hardcoded widget list in this file).
  const entries = useMemo(
    () =>
      getRegisteredTypes().map((type) => {
        const def = getWidget(type);
        if (!def) return null;
        const label = def.titleKey ? t(locale, def.titleKey) : type;
        return { type, label };
      }).filter((entry): entry is { type: string; label: string } => entry !== null),
    [locale],
  );

  // Escape to dismiss + click-outside close (attached only while open).
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onOpenChange(false);
      }
    };
    const onPointerDown = (event: Event) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (menuRef.current?.contains(target)) return;
      if (triggerRef.current?.contains(target)) return;
      onOpenChange(false);
    };
    document.addEventListener('keydown', onKeyDown, true);
    document.addEventListener('mousedown', onPointerDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown, true);
      document.removeEventListener('mousedown', onPointerDown);
    };
  }, [open, onOpenChange, triggerRef]);

  // Return focus to the trigger whenever the menu closes.
  useEffect(() => {
    if (!open) triggerRef.current?.focus();
  }, [open, triggerRef]);

  if (!open) return null;

  const handleSelect = (type: string) => {
    onOpenChange(false);
    onSelect(type);
  };

  // Empty state is safe by construction: the built-in registry is always
  // non-empty at this surface, and an empty list simply renders no items
  // (the menu stays open with zero rows — no crash, no fabricated entries).
  return (
    <div
      ref={menuRef}
      role="menu"
      aria-label={t(locale, 'workspace.workspaceViewsLabel')}
      className="absolute right-0 top-full z-50 mt-1 min-w-44 overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-lg"
    >
      {entries.map((entry) => (
        <button
          key={entry.type}
          type="button"
          role="menuitem"
          onClick={() => handleSelect(entry.type)}
          className="flex w-full items-center gap-1.5 rounded-md px-2.5 py-1.5 text-left text-xs text-[var(--text-secondary)] transition-colors hover:bg-[var(--surface-hover)] hover:text-[var(--text)] focus-visible:outline-2 focus-visible:outline-[var(--primary)]"
        >
          <Plus size={12} className="shrink-0 text-[var(--text-disabled)]" />
          <span className="truncate">{entry.label}</span>
        </button>
      ))}
    </div>
  );
}

export default AddWidgetMenu;
