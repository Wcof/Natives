'use client';

import { useEffect, useId, useRef } from 'react';
import { useHydrated } from '@/hooks/useHydrated';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { useFocusTrap } from '@/lib/useFocusTrap';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title?: string;
  children: ReactNode;
  width?: number;
  showCloseButton?: boolean;
  closeOnBackdropClick?: boolean;
  closeOnEscape?: boolean;
  className?: string;
  contentClassName?: string;
}

export default function Modal({
  isOpen,
  onClose,
  title,
  children,
  width = 480,
  showCloseButton = true,
  closeOnBackdropClick = true,
  closeOnEscape = true,
  className,
  contentClassName,
}: ModalProps) {
  const locale = useLocale();
  const titleId = useId();
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const isMounted = useHydrated();
  const { dialogRef, handleKeyDown } = useFocusTrap();

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    previousFocusRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;

    const focusFrame = window.requestAnimationFrame(() => {
      const firstFocusable = dialogRef.current?.querySelector<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      (firstFocusable ?? dialogRef.current)?.focus();
    });

    return () => {
      window.cancelAnimationFrame(focusFrame);
      previousFocusRef.current?.focus();
      previousFocusRef.current = null;
    };
  }, [dialogRef, isOpen]);

  useEffect(() => {
    if (!isOpen || !closeOnEscape) {
      return;
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };

    document.addEventListener('keydown', handleEscape, true);
    return () => {
      document.removeEventListener('keydown', handleEscape, true);
    };
  }, [closeOnEscape, isOpen, onClose]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    return () => {
      document.body.style.overflow = previousOverflow;
    };
  }, [isOpen]);

  if (!isMounted || !isOpen) {
    return null;
  }

  const showHeader = Boolean(title) || showCloseButton;
  return createPortal(
    <div
      style={{ position: 'fixed', inset: 0, zIndex: 50, display: 'flex', alignItems: 'center', justifyContent: 'center', background: 'var(--overlay)', animation: `fadeIn ${TRANSITION.normal}`, pointerEvents: 'auto' }}
      onMouseDown={(event) => {
        if (
          closeOnBackdropClick &&
          event.target === event.currentTarget
        ) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? titleId : undefined}
        tabIndex={-1}
        className={['anim-dropIn', className].filter(Boolean).join(' ')}
        style={{
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: BORDER_RADIUS.lg,
          boxShadow: 'var(--shadow-modal)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
          maxHeight: '85vh',
          width: `min(${width}px, calc(100vw - 2rem))`,
          outline: 'none',
        }}
        onKeyDown={handleKeyDown}
        onMouseDown={(event) => event.stopPropagation()}
      >
        {showHeader && (
          <div style={{ display: 'flex', flexShrink: 0, alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid var(--border)', padding: `${SPACING.sm}px ${SPACING.lg}px` }}>
            {title ? (
              <h2
                id={titleId}
                style={{ fontFamily: 'var(--font-display, inherit)', fontSize: FONT_SIZE.xl, fontWeight: 600, lineHeight: 1.25, color: 'var(--text)' }}
              >
                {title}
              </h2>
            ) : (
              <span />
            )}
            {showCloseButton && (
              <button
                type="button"
                onClick={onClose}
                aria-label={t(locale, 'common.close')}
                title={t(locale, 'common.close')}
                style={{ display: 'flex', height: '32px', width: '32px', flexShrink: 0, alignItems: 'center', justifyContent: 'center', borderRadius: BORDER_RADIUS.xs, border: 'none', cursor: 'pointer', color: 'var(--text-secondary)', background: 'transparent', transition: `background-color ${TRANSITION.fast}, color ${TRANSITION.fast}` }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--surface-hover)'; (e.currentTarget as HTMLElement).style.color = 'var(--text)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; (e.currentTarget as HTMLElement).style.color = 'var(--text-secondary)'; }}
              >
                <X size={17} />
              </button>
            )}
          </div>
        )}
        <div
          className={[
            'min-h-0 flex-1 overflow-y-auto p-4',
            contentClassName,
          ]
            .filter(Boolean)
            .join(' ')}
        >
          {children}
        </div>
      </div>
    </div>,
    document.getElementById('content-overlay-root') ?? document.body,
  );
}
