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
import {
  applyTheme,
  normalizeThemeId,
  getReducedMotion,
  getReducedTransparency,
  onPreferenceChange,
} from '@/lib/theme-engine';
import type { V2ThemeId } from '@/lib/design-tokens';

// ── AI Natives Design System V2 Theme Context ──
// V2: Dark Glow（暗黑流光）+ Liquid Crystal（晶透液态）。
// 内部 ID 固定 dark / light；旧名 terminal-volt / frosted-jasmine 只读兼容。
// reduced-motion / reduced-transparency 通过 data-* 属性与 CSS fallback 生效。

interface ThemeContextValue {
  themeId: V2ThemeId;
  setTheme: (id: string) => void;
  isDark: boolean;
  /** 系统 prefers-reduced-motion 是否生效（客户端实时）。 */
  reducedMotion: boolean;
  /** 系统 prefers-reduced-transparency 是否生效（客户端实时）。 */
  reducedTransparency: boolean;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

interface ThemeProviderProps {
  children: ReactNode;
}

export function ThemeProvider({ children }: ThemeProviderProps) {
  const [themeId, setThemeId] = useState<V2ThemeId>('dark');
  const [reducedMotion, setReducedMotion] = useState(false);
  const [reducedTransparency, setReducedTransparency] = useState(false);

  // 同步 data-theme 变化（MutationObserver，单一真值 = html[data-theme]）
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

  // 同步系统偏好（motion / transparency）
  useEffect(() => {
    setReducedMotion(getReducedMotion());
    setReducedTransparency(getReducedTransparency());
    return onPreferenceChange(() => {
      setReducedMotion(getReducedMotion());
      setReducedTransparency(getReducedTransparency());
    });
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
      reducedMotion,
      reducedTransparency,
    }),
    [setTheme, themeId, reducedMotion, reducedTransparency],
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

// ── 偏好 Hooks（供 design-system 组件消费，例如 AnimatedMetric 尊重 reduced motion） ──

export function useReducedMotion(): boolean {
  return useTheme().reducedMotion;
}

export function useReducedTransparency(): boolean {
  return useTheme().reducedTransparency;
}

// ── 向后兼容：V1 废弃导出 ──
// Canvas quota / Liquid Glass config 在 V2 中永久 no-op（V1.0 已禁用 WebGL）。
interface CanvasQuota {
  allowed: boolean;
  release: () => void;
}

export function useCanvasQuota(_enabled = true): CanvasQuota {
  const release = useCallback(() => {}, []);
  return { allowed: false, release };
}

export const applyLiquidGlassConfig = (_config: {
  blurAmount?: number;
  saturation?: number;
  cornerRadius?: number;
}) => {
  // V2 no-op: 玻璃强度统一走语义 token，不动态注入 WebGL 配置。
};

export const MAX_CONCURRENT_WEBGL_CANVASES = 0;
