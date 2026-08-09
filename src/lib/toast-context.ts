'use client';

import { createContext, useContext } from 'react';

/**
 * Toast context contract (W4): split out of `components/ui/Toast.tsx` so hooks
 * can consume `useToast` without depending on a component module. The UI
 * component owns the provider + rendering and re-exports these for
 * backwards compatibility.
 */

export type ToastType = 'info' | 'success' | 'error' | 'warning';

export interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
  dismissing: boolean;
}

export interface ToastContextValue {
  toast: (message: string, type?: ToastType) => void;
}

export const ToastContext = createContext<ToastContextValue>({ toast: () => {} });

export function useToast(): ToastContextValue {
  return useContext(ToastContext);
}
