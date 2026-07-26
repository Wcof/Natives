'use client';

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react';
import type { ReactNode } from 'react';
import { applyTheme, normalizeThemeId } from '@/lib/theme-engine';

// ── AI Natives V1.0 Theme Context ──
// 已移除 Liquid Glass / WebGL canvas quota / backdrop-filter 动态注入。
// V1.0 不使用 backdrop-filter，纯色 Surface + 轻边框。

interface ThemeContextValue {
  themeId: string;
  setTheme: (id: string) => void;
  isDark: boolean;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

interface ThemeProviderProps {
  children: ReactNode;
}

export function ThemeProvider({ children }: ThemeProviderProps) {
  // 与 theme-engine 的 normalizeThemeId 兜底、后端 DEFAULT_THEME（terminal-volt→dark）
  // 及 layout.tsx 的 SSR 初值统一为 dark，消除首帧闪变与三处漂移
  const [themeId, setThemeId] = useState('dark');

  useEffect(() => {
    const root = document.documentElement;
    const syncThemeId = () => {
      setThemeId(normalizeThemeId(root.getAttribute('data-theme')));
    };

    syncThemeId();
    const observer = new MutationObserver(syncThemeId);
    observer.observe(root, { attributes: true, attributeFilter: ['data-theme'] });

    return () => observer.disconnect();
  }, []);

  const setTheme = useCallback((id: string) => {
    const nextTheme = normalizeThemeId(id);
    applyTheme(nextTheme);
    setThemeId(nextTheme);
    window.nativesAPI?.setTheme?.(nextTheme).catch(() => {});
  }, []);

  const value = useMemo<ThemeContextValue>(
    () => ({
      themeId,
      setTheme,
      isDark: themeId === 'dark',
    }),
    [setTheme, themeId],
  );

  return (
    <ThemeContext.Provider value={value}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);

  if (!context) {
    throw new Error('useTheme must be used within a ThemeProvider.');
  }

  return context;
}

// ── Canvas quota 已废弃（V1.0 无 WebGL） ──
// 保留导出以避免破坏旧 import，但永久返回 allowed: false。
interface CanvasQuota {
  allowed: boolean;
  release: () => void;
}

export function useCanvasQuota(_enabled = true): CanvasQuota {
  const release = useCallback(() => {}, []);
  return { allowed: false, release };
}

// ── applyLiquidGlassConfig 已废弃（V1.0 无 Liquid Glass） ──
// 保留导出以避免破坏旧 import，但为空操作。
export const applyLiquidGlassConfig = (_config: {
  blurAmount?: number;
  saturation?: number;
  cornerRadius?: number;
}) => {
  // V1.0 no-op: backdrop-filter 已禁用
};

// 向后兼容：WebGL canvas 配额常量
export const MAX_CONCURRENT_WEBGL_CANVASES = 0;
