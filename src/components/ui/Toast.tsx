'use client';

import { useState, useCallback, useEffect, createContext, useContext, useRef } from 'react';
import { XCircle, CheckCircle, AlertTriangle } from 'lucide-react';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

export type ToastType = 'info' | 'success' | 'error' | 'warning';

interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
  dismissing: boolean;
}

interface ToastContextValue {
  toast: (message: string, type?: ToastType) => void;
}

const ToastContext = createContext<ToastContextValue>({ toast: () => {} });

let nextId = 0;

/**
 * Toast Provider — wraps the app and provides a global toast() function.
 * Usage: const { toast } = useToast(); toast('Saved!', 'success');
 */
export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const timersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());

  const removeToast = useCallback((id: number) => {
    // Start dismiss animation
    setToasts(prev => prev.map(t => t.id === id ? { ...t, dismissing: true } : t));
    // Remove after animation
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id));
      timersRef.current.delete(id);
    }, 300);
  }, []);

  const toast = useCallback((message: string, type: ToastType = 'info') => {
    const id = nextId++;
    setToasts(prev => [...prev, { id, message, type, dismissing: false }]);
    // Auto-dismiss after 3s
    const timer = setTimeout(() => removeToast(id), 3000);
    timersRef.current.set(id, timer);
  }, [removeToast]);

  // Cleanup timers on unmount
  useEffect(() => {
    return () => {
      timersRef.current.forEach(timer => clearTimeout(timer));
    };
  }, []);

  return (
    <ToastContext.Provider value={{ toast }}>
      {children}
      {/* Toast container — bottom-right */}
      <div style={{
        position: 'fixed', bottom: SPACING.lg, right: SPACING.lg, zIndex: 10001,
        display: 'flex', flexDirection: 'column-reverse', gap: SPACING.sm,
        pointerEvents: 'none',
      }}>
        {toasts.map(t => (
          <div
            key={t.id}
            style={{
              padding: `${SPACING.sm}px ${SPACING.lg}px`, borderRadius: BORDER_RADIUS.lg, fontSize: FONT_SIZE.lg, fontWeight: 500,
              color: 'var(--accent-ink)',
              background: t.type === 'success' ? 'var(--diff-add)' : t.type === 'error' ? 'var(--danger)' : t.type === 'warning' ? 'var(--warning)' : 'var(--accent)',
              boxShadow: '0 4px 16px rgba(0,0,0,0.3)',
              opacity: t.dismissing ? 0 : 1,
              transform: t.dismissing ? 'translateY(8px)' : 'translateY(0)',
              transition: `opacity ${TRANSITION.slow}, transform ${TRANSITION.slow}`,
              pointerEvents: 'auto',
              cursor: 'pointer',
              maxWidth: 360,
              display: 'inline-flex', alignItems: 'center', gap: SPACING.xs,
            }}
            onClick={() => removeToast(t.id)}
          >
            {t.type === 'error' && <XCircle size={14} style={{ flexShrink: 0 }} />}
            {t.type === 'success' && <CheckCircle size={14} style={{ flexShrink: 0 }} />}
            {t.type === 'warning' && <AlertTriangle size={14} style={{ flexShrink: 0 }} />}
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{t.message}</span>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  return useContext(ToastContext);
}
