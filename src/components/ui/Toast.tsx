'use client';

import { useState, useCallback, useEffect, createContext, useContext, useRef } from 'react';

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
        position: 'fixed', bottom: 16, right: 16, zIndex: 10001,
        display: 'flex', flexDirection: 'column-reverse', gap: 8,
        pointerEvents: 'none',
      }}>
        {toasts.map(t => (
          <div
            key={t.id}
            style={{
              padding: '8px 16px', borderRadius: 8, fontSize: 13, fontWeight: 500,
              color: 'var(--bg, #0b0c0a)',
              background: t.type === 'success' ? '#4ec9b0' : t.type === 'error' ? '#d9534f' : t.type === 'warning' ? '#e6b800' : 'var(--accent, #cdf24b)',
              boxShadow: '0 4px 16px rgba(0,0,0,0.3)',
              opacity: t.dismissing ? 0 : 1,
              transform: t.dismissing ? 'translateY(8px)' : 'translateY(0)',
              transition: 'opacity 0.3s ease, transform 0.3s ease',
              pointerEvents: 'auto',
              cursor: 'pointer',
              maxWidth: 360,
              overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
            }}
            onClick={() => removeToast(t.id)}
          >
            {t.type === 'error' ? '❌ ' : t.type === 'success' ? '✅ ' : t.type === 'warning' ? '⚠️ ' : ''}
            {t.message}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  return useContext(ToastContext);
}
