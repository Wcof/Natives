'use client';

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { ReactNode } from 'react';

export const MAX_CONCURRENT_WEBGL_CANVASES = 2;

interface ThemeContextValue {
  themeId: string;
  setTheme: (id: string) => void;
  isDark: boolean;
  registerCanvas: () => boolean;
  unregisterCanvas: () => void;
  activeCanvasCount: number;
}

interface CanvasQuota {
  allowed: boolean;
  release: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

interface ThemeProviderProps {
  children: ReactNode;
}

/**
 * Computes and sets all computed CSS variables directly on document.documentElement.
 * This bypasses Chromium's nested custom properties performance optimization bug,
 * forcing an immediate redraw/repaint of GPU-accelerated backdrop filters and border radii.
 */
export const applyLiquidGlassConfig = (config: {
  blurAmount?: number;
  saturation?: number;
  cornerRadius?: number;
}) => {
  const root = document.documentElement;
  const blurAmount = typeof config.blurAmount === 'number' ? config.blurAmount : 0.40;
  const saturation = typeof config.saturation === 'number' ? config.saturation : 135;
  const cornerRadius = typeof config.cornerRadius === 'number' ? config.cornerRadius : 28;

  const blurFactor = blurAmount / 0.40;
  const saturationFactor = saturation / 135;
  const radiusFactor = cornerRadius / 28;

  root.style.setProperty('--liquid-blur-factor', String(blurFactor));
  root.style.setProperty('--liquid-saturation-factor', String(saturationFactor));
  root.style.setProperty('--liquid-radius-factor', String(radiusFactor));

  // Directly set the computed variables to force Chromium to repaint GPU-accelerated backdrop filters
  root.style.setProperty('--vibe-sidebar-blur', `${28 * blurFactor}px`);
  root.style.setProperty('--vibe-sidebar-saturation', `${145 * saturationFactor}%`);
  root.style.setProperty('--vibe-sidebar-radius', `${18 * radiusFactor}px`);

  root.style.setProperty('--vibe-toolbar-blur', `${22 * blurFactor}px`);
  root.style.setProperty('--vibe-toolbar-saturation', `${145 * saturationFactor}%`);
  root.style.setProperty('--vibe-toolbar-radius', `${16 * radiusFactor}px`);

  root.style.setProperty('--vibe-content-blur', `${24 * blurFactor}px`);
  root.style.setProperty('--vibe-content-saturation', `${145 * saturationFactor}%`);
  root.style.setProperty('--vibe-content-radius', `${16 * radiusFactor}px`);

  root.style.setProperty('--vibe-right-panel-blur', `${28 * blurFactor}px`);
  root.style.setProperty('--vibe-right-panel-saturation', `${145 * saturationFactor}%`);
  root.style.setProperty('--vibe-right-panel-radius', `${18 * radiusFactor}px`);

  root.style.setProperty('--shell-blur', `${40 * blurFactor}px`);
  root.style.setProperty('--shell-saturation', `${210 * saturationFactor}%`);

  root.style.setProperty('--panel-header-blur', `${20 * blurFactor}px`);
  root.style.setProperty('--panel-header-saturation', `${140 * saturationFactor}%`);

  root.style.setProperty('--context-menu-blur', `${20 * blurFactor}px`);
  root.style.setProperty('--context-menu-saturation', `${140 * saturationFactor}%`);

  root.style.setProperty('--glass-overlay-blur', `${24 * blurFactor}px`);
  root.style.setProperty('--glass-overlay-saturation', `${150 * saturationFactor}%`);

  root.style.setProperty('--glass-blur', `${28 * blurFactor}px`);
  root.style.setProperty('--glass-saturation', `${170 * saturationFactor}%`);
  root.style.setProperty('--glass-radius', `${16 * radiusFactor}px`);

  root.style.setProperty('--main-card-blur', `${200 * blurFactor}px`);
  root.style.setProperty('--main-card-saturation', `${280 * saturationFactor}%`);
  root.style.setProperty('--main-card-radius', `${28 * radiusFactor}px`);
};

export function ThemeProvider({ children }: ThemeProviderProps) {
  const [themeId, setThemeId] = useState('terminal-volt');
  const [activeCanvasCount, setActiveCanvasCount] = useState(0);
  const canvasCountRef = useRef(0);

  useEffect(() => {
    const root = document.documentElement;
    const syncThemeId = () => {
      setThemeId(root.getAttribute('data-theme') ?? 'terminal-volt');
    };

    syncThemeId();
    const observer = new MutationObserver(syncThemeId);
    observer.observe(root, { attributes: true, attributeFilter: ['data-theme'] });

    return () => observer.disconnect();
  }, []);

  const setTheme = useCallback((id: string) => {
    document.documentElement.setAttribute('data-theme', id);
    setThemeId(id);
  }, []);

  const registerCanvas = useCallback((): boolean => {
    if (canvasCountRef.current >= MAX_CONCURRENT_WEBGL_CANVASES) {
      return false;
    }

    const nextCount = canvasCountRef.current + 1;
    canvasCountRef.current = nextCount;
    setActiveCanvasCount(nextCount);
    return true;
  }, []);

  const unregisterCanvas = useCallback((): void => {
    if (canvasCountRef.current === 0) {
      return;
    }

    const nextCount = canvasCountRef.current - 1;
    canvasCountRef.current = nextCount;
    setActiveCanvasCount(nextCount);
  }, []);

  const value = useMemo<ThemeContextValue>(
    () => ({
      themeId,
      setTheme,
      isDark: themeId === 'terminal-volt',
      registerCanvas,
      unregisterCanvas,
      activeCanvasCount,
    }),
    [
      activeCanvasCount,
      registerCanvas,
      setTheme,
      themeId,
      unregisterCanvas,
    ],
  );

  const [visualConfig, setVisualConfig] = useState({
    blurAmount: 0.40,
    saturation: 135,
    cornerRadius: 28,
  });

  const CONFIG_DB_KEY = 'settings:controlHubVisuals';

  useEffect(() => {
    const loadConfig = async () => {
      try {
        const api = window.nativesAPI;
        if (!api?.db?.get) return;
        const savedStr = await api.db.get(CONFIG_DB_KEY);
        if (savedStr) {
          try {
            const config = JSON.parse(savedStr as string);
            if (config) {
              setVisualConfig({
                blurAmount: typeof config.blurAmount === 'number' ? config.blurAmount : 0.40,
                saturation: typeof config.saturation === 'number' ? config.saturation : 135,
                cornerRadius: typeof config.cornerRadius === 'number' ? config.cornerRadius : 28,
              });
              applyLiquidGlassConfig(config);
            }
          } catch { /* no-op */ }
        }
      } catch (error) {
        console.warn('[ThemeContext] Failed to load liquid glass config:', error);
      }
    };

    loadConfig();

    let unsubscribe: (() => void) | undefined;

    try {
      if (window.nativesAPI?.onDbStateChanged) {
        unsubscribe = window.nativesAPI.onDbStateChanged(
          (_event: unknown, channel: string, data: any) => {
            if (channel === 'module_data' && data?.key === CONFIG_DB_KEY) {
              loadConfig();
            }
          },
        );
      }
    } catch (error) {
      console.warn('[ThemeContext] Failed to subscribe to onDbStateChanged:', error);
    }

    return () => {
      if (unsubscribe) {
        unsubscribe();
      }
    };
  }, []);

  // Listen to visual-config-changed custom events for zero-latency slider sync
  useEffect(() => {
    const handleLocalConfigChange = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail) {
        setVisualConfig({
          blurAmount: typeof detail.blurAmount === 'number' ? detail.blurAmount : 0.40,
          saturation: typeof detail.saturation === 'number' ? detail.saturation : 135,
          cornerRadius: typeof detail.cornerRadius === 'number' ? detail.cornerRadius : 28,
        });
      }
    };
    window.addEventListener('visual-config-changed', handleLocalConfigChange);
    return () => {
      window.removeEventListener('visual-config-changed', handleLocalConfigChange);
    };
  }, []);

  return (
    <ThemeContext.Provider value={value}>
      <DynamicGlassStyle config={visualConfig} />
      {children}
    </ThemeContext.Provider>
  );
}

function DynamicGlassStyle({ config }: { config: { blurAmount: number; saturation: number; cornerRadius: number } }) {
  const blurFactor = config.blurAmount / 0.40;
  const saturationFactor = config.saturation / 135;
  const radiusFactor = config.cornerRadius / 28;

  const sidebarBlur = 28 * blurFactor;
  const sidebarSaturation = 145 * saturationFactor;
  const sidebarRadius = 18 * radiusFactor;

  const toolbarBlur = 22 * blurFactor;
  const toolbarSaturation = 145 * saturationFactor;
  const toolbarRadius = 16 * radiusFactor;

  const contentBlur = 24 * blurFactor;
  const contentSaturation = 145 * saturationFactor;
  const contentRadius = 16 * radiusFactor;

  const rightPanelBlur = 28 * blurFactor;
  const rightPanelSaturation = 145 * saturationFactor;
  const rightPanelRadius = 18 * radiusFactor;

  const contextMenuBlur = 20 * blurFactor;
  const contextMenuSaturation = 140 * saturationFactor;

  const glassOverlayBlur = 24 * blurFactor;
  const glassOverlaySaturation = 150 * saturationFactor;

  const mainCardBlur = 200 * blurFactor;
  const mainCardSaturation = 280 * saturationFactor;
  const mainCardRadius = 28 * radiusFactor;

  const css = `
    :root {
      --vibe-sidebar-blur: ${sidebarBlur}px !important;
      --vibe-sidebar-saturation: ${sidebarSaturation}% !important;
      --vibe-sidebar-radius: ${sidebarRadius}px !important;

      --vibe-toolbar-blur: ${toolbarBlur}px !important;
      --vibe-toolbar-saturation: ${toolbarSaturation}% !important;
      --vibe-toolbar-radius: ${toolbarRadius}px !important;

      --vibe-content-blur: ${contentBlur}px !important;
      --vibe-content-saturation: ${contentSaturation}% !important;
      --vibe-content-radius: ${contentRadius}px !important;

      --vibe-right-panel-blur: ${rightPanelBlur}px !important;
      --vibe-right-panel-saturation: ${rightPanelSaturation}% !important;
      --vibe-right-panel-radius: ${rightPanelRadius}px !important;

      --context-menu-blur: ${contextMenuBlur}px !important;
      --context-menu-saturation: ${contextMenuSaturation}% !important;

      --glass-overlay-blur: ${glassOverlayBlur}px !important;
      --glass-overlay-saturation: ${glassOverlaySaturation}% !important;

      --glass-blur: ${sidebarBlur}px !important;
      --glass-saturation: ${sidebarSaturation}% !important;
      --glass-radius: ${sidebarRadius}px !important;

      --main-card-blur: ${mainCardBlur}px !important;
      --main-card-saturation: ${mainCardSaturation}% !important;
      --main-card-radius: ${mainCardRadius}px !important;
    }

    .vibe-sidebar {
      backdrop-filter: blur(${sidebarBlur}px) saturate(${sidebarSaturation}%) !important;
      -webkit-backdrop-filter: blur(${sidebarBlur}px) saturate(${sidebarSaturation}%) !important;
      border-radius: ${sidebarRadius}px !important;
    }
    .vibe-toolbar {
      backdrop-filter: blur(${toolbarBlur}px) saturate(${toolbarSaturation}%) !important;
      -webkit-backdrop-filter: blur(${toolbarBlur}px) saturate(${toolbarSaturation}%) !important;
      border-radius: ${toolbarRadius}px !important;
    }
    .vibe-content-panel {
      backdrop-filter: blur(${contentBlur}px) saturate(${contentSaturation}%) !important;
      -webkit-backdrop-filter: blur(${contentBlur}px) saturate(${contentSaturation}%) !important;
      border-radius: ${contentRadius}px !important;
    }
    .vibe-right-panel {
      backdrop-filter: blur(${rightPanelBlur}px) saturate(${rightPanelSaturation}%) !important;
      -webkit-backdrop-filter: blur(${rightPanelBlur}px) saturate(${rightPanelSaturation}%) !important;
      border-radius: ${rightPanelRadius}px !important;
    }
    .context-menu {
      backdrop-filter: blur(${contextMenuBlur}px) saturate(${contextMenuSaturation}%) !important;
      -webkit-backdrop-filter: blur(${contextMenuBlur}px) saturate(${contextMenuSaturation}%) !important;
    }
    .glass-overlay {
      backdrop-filter: blur(${glassOverlayBlur}px) saturate(${glassOverlaySaturation}%) !important;
      -webkit-backdrop-filter: blur(${glassOverlayBlur}px) saturate(${glassOverlaySaturation}%) !important;
    }
    .main-card {
      backdrop-filter: blur(${mainCardBlur}px) saturate(${mainCardSaturation}%) !important;
      -webkit-backdrop-filter: blur(${mainCardBlur}px) saturate(${mainCardSaturation}%) !important;
      border-radius: ${mainCardRadius}px !important;
    }
  `;

  return <style dangerouslySetInnerHTML={{ __html: css }} />;
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);

  if (!context) {
    throw new Error('useTheme must be used within a ThemeProvider.');
  }

  return context;
}

export function useCanvasQuota(enabled = true): CanvasQuota {
  const {
    activeCanvasCount,
    registerCanvas,
    unregisterCanvas,
  } = useTheme();
  const acquiredRef = useRef(false);
  const [allowed, setAllowed] = useState(false);

  const release = useCallback(() => {
    if (!acquiredRef.current) {
      return;
    }

    acquiredRef.current = false;
    unregisterCanvas();
    setAllowed(false);
  }, [unregisterCanvas]);

  const tryAcquire = useCallback(() => {
    if (!enabled || acquiredRef.current) {
      return;
    }

    const granted = registerCanvas();
    acquiredRef.current = granted;
    setAllowed(granted);
  }, [enabled, registerCanvas]);

  useEffect(() => {
    if (enabled) {
      tryAcquire();
    } else {
      release();
    }
  }, [enabled, release, tryAcquire]);

  useEffect(() => {
    if (
      enabled &&
      !acquiredRef.current &&
      activeCanvasCount < MAX_CONCURRENT_WEBGL_CANVASES
    ) {
      tryAcquire();
    }
  }, [activeCanvasCount, enabled, tryAcquire]);

  useEffect(() => release, [release]);

  return { allowed, release };
}
