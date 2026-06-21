'use client';

import { useEffect, useId, useRef } from 'react';
import { useHydrated } from '@/hooks/useHydrated';
import type { CSSProperties, ReactNode } from 'react';
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

type ModalWidthStyle = CSSProperties & {
  '--modal-width': string;
};

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
  const widthStyle: ModalWidthStyle = {
    '--modal-width': `${width}px`,
  };

  return createPortal(
    <div
      style={{ position: 'absolute', inset: 0, zIndex: 50, display: 'flex', alignItems: 'center', justifyContent: 'center', background: 'rgba(0,0,0,0.45)', backdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))', WebkitBackdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))', animation: `fadeIn ${TRANSITION.normal}` }}
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
          ...widthStyle,
          background: 'var(--vibe-toolbar-bg)',
          backdropFilter: 'blur(var(--vibe-sidebar-blur, 28px)) saturate(var(--vibe-sidebar-saturation, 145%))',
          WebkitBackdropFilter: 'blur(var(--vibe-sidebar-blur, 28px)) saturate(var(--vibe-sidebar-saturation, 145%))',
          border: '0.0625rem solid var(--vibe-toolbar-border)',
          borderRadius: 'var(--radius)',
          boxShadow: '0 0 0 1px color-mix(in srgb, var(--accent) 12%, transparent), var(--vibe-toolbar-shadow)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
          maxHeight: '85vh',
          width: `min(var(--modal-width),calc(100vw-2rem))`,
          outline: 'none',
        }}
        onKeyDown={handleKeyDown}
        onMouseDown={(event) => event.stopPropagation()}
      >
        {showHeader && (
          <div style={{ display: 'flex', flexShrink: 0, alignItems: 'center', justifyContent: 'space-between', borderBottom: '0.0625rem solid var(--vibe-toolbar-border)', padding: `${SPACING.sm}px ${SPACING.lg}px` }}>
            {title ? (
              <h2
                id={titleId}
                style={{ fontFamily: 'var(--vibe-font-display, inherit)', fontSize: FONT_SIZE.xl, fontWeight: 600, lineHeight: 1.25, color: 'var(--vibe-accent)' }}
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
                style={{ display: 'flex', height: '2rem', width: '2rem', flexShrink: 0, alignItems: 'center', justifyContent: 'center', borderRadius: BORDER_RADIUS.lg, border: 'none', cursor: 'pointer', color: 'var(--vibe-btn-text)', background: 'transparent', transition: `all ${TRANSITION.fast}` }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)'; (e.currentTarget as HTMLElement).style.color = 'var(--vibe-accent)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; (e.currentTarget as HTMLElement).style.color = 'var(--vibe-btn-text)'; }}
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
