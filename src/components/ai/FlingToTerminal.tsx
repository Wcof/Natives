'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import { Zap } from 'lucide-react';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface FlingToTerminalProps {
  /** Ref to the terminal panel element for targeting the flight animation */
  terminalRef?: React.RefObject<HTMLElement | null>;
  /** Callback to write text to the active terminal session */
  onFling?: (text: string) => void;
}

/**
 * Fling to Terminal — select text in file preview, fly it to the terminal.
 *
 * Inspired by fanbox's fling-to-terminal pattern:
 * - Shows "Send to Terminal" button near text selection
 * - Flight animation with cubic-bezier curve
 * - Terminal catches with accent glow
 * - Uses bracketed paste for multi-line content
 */
export default function FlingToTerminal({ terminalRef, onFling }: FlingToTerminalProps) {
  const [selection, setSelection] = useState<{ text: string; x: number; y: number } | null>(null);
  const [flying, setFlying] = useState(false);
  const [flyTarget, setFlyTarget] = useState({ x: 0, y: 0 });
  const ghostRef = useRef<HTMLButtonElement>(null);

  // Listen for text selection changes
  useEffect(() => {
    const handleMouseUp = () => {
      const sel = window.getSelection();
      if (!sel || sel.isCollapsed || !sel.toString().trim()) {
        setSelection(null);
        return;
      }

      const text = sel.toString().trim();
      if (text.length < 2) return;

      // Position the button near the selection
      const range = sel.getRangeAt(0);
      const rect = range.getBoundingClientRect();
      setSelection({
        text,
        x: rect.left + rect.width / 2,
        y: rect.top - 8,
      });
    };

    // Clear on click outside
    const handleMouseDown = (e: MouseEvent) => {
      if (ghostRef.current && !ghostRef.current.contains(e.target as Node)) {
        if (!flying) setSelection(null);
      }
    };

    document.addEventListener('mouseup', handleMouseUp);
    document.addEventListener('mousedown', handleMouseDown);
    return () => {
      document.removeEventListener('mouseup', handleMouseUp);
      document.removeEventListener('mousedown', handleMouseDown);
    };
  }, [flying]);

  const handleFling = useCallback(() => {
    if (!selection || flying) return;

    // Get terminal panel position for flight target
    const termEl = terminalRef?.current;
    const targetRect = termEl?.getBoundingClientRect();
    const tx = targetRect ? targetRect.left + targetRect.width / 2 : window.innerWidth / 2;
    const ty = targetRect ? targetRect.top + 40 : window.innerHeight - 100;

    setFlyTarget({ x: tx, y: ty });
    setFlying(true);

    // After animation completes (550ms), send text to terminal
    setTimeout(() => {
      onFling?.(selection.text);
      setFlying(false);
      setSelection(null);

      // Flash terminal catch glow
      if (termEl) {
        termEl.classList.add('anim-termCatch');
        setTimeout(() => termEl.classList.remove('anim-termCatch'), 500);
      }
    }, 550);
  }, [selection, flying, terminalRef, onFling]);

  if (!selection && !flying) return null;

  return (
    <>
      {/* Send to Terminal button — appears near selection */}
      {selection && !flying && (
        <button
          ref={ghostRef}
          onClick={handleFling}
          style={{
            position: 'fixed',
            left: selection.x,
            top: selection.y,
            transform: 'translate(-50%, -100%)',
            zIndex: 9999,
            background: 'var(--accent)',
            color: 'var(--vibe-content-bg)',
            border: 'none',
            borderRadius: BORDER_RADIUS.md,
            padding: '4px 10px',
            fontSize: FONT_SIZE.sm,
            fontWeight: 600,
            cursor: 'pointer',
            boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
            animation: 'fadeIn 0.15s ease',
            whiteSpace: 'nowrap',
          }}
        >
          <Zap size={14} /> Send to Terminal
        </button>
      )}

      {/* Flying ghost element */}
      {flying && selection && (
        <div
          style={{
            position: 'fixed',
            left: selection.x,
            top: selection.y,
            transform: 'translate(-50%, -100%)',
            zIndex: 10000,
            background: 'var(--accent)',
            color: 'var(--vibe-content-bg)',
            borderRadius: BORDER_RADIUS.md,
            padding: '4px 10px',
            fontSize: FONT_SIZE.sm,
            fontWeight: 600,
            maxWidth: 280,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            boxShadow: '0 4px 16px var(--accent-soft, #cdf24b44)',
            pointerEvents: 'none',
            transition: 'transform 0.55s cubic-bezier(0.45, 0, 0.2, 1), opacity 0.55s ease',
            // Animate to terminal position
            ...(flying ? {
              transform: `translate(${flyTarget.x - selection.x}px, ${flyTarget.y - selection.y}px) scale(0.25) rotate(7deg)`,
              opacity: 0,
            } : {}),
          }}
        >
          {selection.text.length > 42 ? selection.text.slice(0, 42) + '…' : selection.text}
        </div>
      )}
    </>
  );
}
