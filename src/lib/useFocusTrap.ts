'use client';

import { useRef, useCallback } from 'react';

/**
 * Shared focus-trap hook (STYLE-2).
 * Returns a ref to attach to the dialog container and a keyDown handler.
 *
 * Usage:
 *   const { dialogRef, handleKeyDown } = useFocusTrap();
 *   return <div ref={dialogRef} onKeyDown={handleKeyDown} role="dialog" aria-modal="true">...</div>
 */
export function useFocusTrap() {
  const dialogRef = useRef<HTMLDivElement>(null);

  const getFocusableElements = useCallback(() => {
    if (!dialogRef.current) return [];
    return Array.from(
      dialogRef.current.querySelectorAll<HTMLElement>(
        'input, [role="option"], button, [tabindex]:not([tabindex="-1"])',
      ),
    );
  }, []);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Tab') {
      const focusable = getFocusableElements();
      if (focusable.length === 0) return;
      e.preventDefault();
      const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
      if (e.shiftKey) {
        const prev = currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1;
        focusable[prev]?.focus();
      } else {
        const next = currentIndex >= focusable.length - 1 ? 0 : currentIndex + 1;
        focusable[next]?.focus();
      }
    }
  }, [getFocusableElements]);

  return { dialogRef, handleKeyDown };
}
