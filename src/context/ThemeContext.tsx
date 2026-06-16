'use client';

import { createContext, useContext, useState, useCallback, useRef, useEffect } from 'react';
import type { ReactNode } from 'react';

// ── Types ──

interface ThemeContextValue {
  themeId: string;
  setTheme: (id: string) => void;
  isDark: boolean;
  registerCanvas: () => boolean;
  unregisterCanvas: () => void;
  activeCanvasCount: number;
}

interface UseCanvasQuotaReturn {
  allowed: boolean;
  release: () => void;
}

// ── Constants ──

const MAX_CONCURRENT_CANVASES = 2;

// ── Context ──

const ThemeContext = createContext<ThemeContextValue | null>(null);

// ── Provider ──

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [themeId, setThemeId] = useState<string>('dark');
  const [activeCanvasCount, setActiveCanvasCount] = useState(0);
  const canvasCountRef = useRef(0);

  // Read initial theme from DOM
  useEffect(() => {
    const initial = document.documentElement.getAttribute('data-theme') || 'dark';
    setThemeId(initial);
  }, []);

  const setTheme = useCallback((id: string) => {
    document.documentElement.setAttribute('data-theme', id);
    setThemeId(id);
  }, []);

  const isDark = themeId === 'liquid-glass';

  const registerCanvas = useCallback((): boolean => {
    if (canvasCountRef.current >= MAX_CONCURRENT_CANVASES) {
      return false;
    }
    canvasCountRef.current += 1;
    setActiveCanvasCount(canvasCountRef.current);
    return true;
  }, []);

  const unregisterCanvas = useCallback(() => {
    canvasCountRef.current = Math.max(0, canvasCountRef.current - 1);
    setActiveCanvasCount(canvasCountRef.current);
  }, []);

  return (
    <ThemeContext.Provider
      value={{
        themeId,
        setTheme,
        isDark,
        registerCanvas,
        unregisterCanvas,
        activeCanvasCount,
      }}
    >
      {children}
    </ThemeContext.Provider>
  );
}

// ── useTheme hook ──

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error('useTheme must be used within a <ThemeProvider>');
  }
  return ctx;
}

// ── useCanvasQuota hook ──

export function useCanvasQuota(): UseCanvasQuotaReturn {
  const { registerCanvas, unregisterCanvas } = useTheme();
  const registeredRef = useRef(false);
  const [allowed, setAllowed] = useState(false);

  useEffect(() => {
    if (!registeredRef.current) {
      const ok = registerCanvas();
      registeredRef.current = true;
      setAllowed(ok);
    }

    return () => {
      if (registeredRef.current) {
        unregisterCanvas();
        registeredRef.current = false;
        setAllowed(false);
      }
    };
  }, [registerCanvas, unregisterCanvas]);

  const release = useCallback(() => {
    if (registeredRef.current) {
      unregisterCanvas();
      registeredRef.current = false;
      setAllowed(false);
    }
  }, [unregisterCanvas]);

  return { allowed, release };
}
