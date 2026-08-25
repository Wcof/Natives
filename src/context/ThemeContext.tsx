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
import {
  getReducedMotion,
  getReducedTransparency,
  onPreferenceChange,
  normalizeThemeId,
  applyTheme,
} from '@/lib/theme-engine';
import {
  classifyThemeError,
  type AppearanceCoordinator,
  type AppearanceSnapshot,
  type ThemeHost,
} from '@/lib/appearance';
import type { V2ThemeId } from '@/lib/design-tokens';

// ── AI Natives Design System V2 Theme Context ──
// 主题语义真值 = AppearanceCoordinator（单一协调权威，TH-02）。
// DOM（html[data-theme]）只是 coordinator 的「输出 sink」——本 Provider 不再
// 反向监听 DOM（旧 MutationObserver 模式已移除）。
//
// · themeId 来自 coordinator.getSnapshot()（由 bootstrap/select/Host 广播驱动）。
// · setTheme 委托 coordinator.select()（Host 持久化成功后才改 DOM + 通知订阅者）。
// · reduced-motion / reduced-transparency 仍走系统偏好实时同步。

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
  /** 测试/嵌入式注入：替代生产 Host facade（同一协调器实例）。 */
  coordinator?: AppearanceCoordinator;
  /** 生产可注入主题 Host（默认经 tauri facade 解析）。 */
  themeHost?: ThemeHost;
}

/** 返回 Provider 级协调器：优先注入；否则解析/复用全局单例。 */
async function resolveProviderCoordinator(
  injected?: AppearanceCoordinator,
  host?: ThemeHost,
): Promise<AppearanceCoordinator> {
  if (injected) return injected;
  const { getAppearanceCoordinator } = await import('@/lib/appearance');
  return getAppearanceCoordinator(host);
}

export function ThemeProvider({ children, coordinator, themeHost }: ThemeProviderProps) {
  // 协调器引用：一旦解析即固定；setTheme 与订阅读取它（避免闭包陈旧）。
  const coordinatorRef = useRef<AppearanceCoordinator | null>(coordinator ?? null);
  const unsubRef = useRef<(() => void) | null>(null);
  const [snapshot, setSnapshot] = useState<AppearanceSnapshot>(() =>
    coordinator ? coordinator.getSnapshot() : { theme: 'dark', revision: 0 },
  );
  const [reducedMotion, setReducedMotionState] = useState(false);
  const [reducedTransparency, setReducedTransparencyState] = useState(false);

  // 首次挂载：解析/创建协调器 → bootstrap（FOUC guard 语义）→ 订阅变更。
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const c = await resolveProviderCoordinator(coordinator, themeHost);
      if (cancelled) return;
      coordinatorRef.current = c;
      setSnapshot(c.getSnapshot());
      if (cancelled) return;

      try {
        const next = await c.bootstrap();
        if (!cancelled) setSnapshot(next);
      } catch (err) {
        // bootstrap 失败已用受控 dark fallback 显窗；此处仅结构化记录，
        // 不 console-only、不抛裸异常（R-F5/E12）。上层可经分类结果展示重试。
        try {
          classifyThemeError(err);
        } catch {
          // 分类本身不可失败：兜底忽略。
        }
        if (!cancelled) setSnapshot(c.getSnapshot());
      }

      // 卸载于 bootstrap 过程中：不再订阅（防泄漏 / 状态更新）。
      if (cancelled) return;

      // 订阅协调器快照（本地 select / Host theme 广播 / 多窗口一致）。
      const unsub = c.subscribe((next: AppearanceSnapshot) => {
        if (!cancelled) setSnapshot(next);
      });
      unsubRef.current = unsub;
    })().catch(() => {
      // resolve 失败（dev 模式无 tauri facade）：保留默认 dark 快照。
      if (!cancelled) setSnapshot((prev) => prev);
    });
    return () => {
      cancelled = true;
      if (unsubRef.current) {
        unsubRef.current();
        unsubRef.current = null;
      }
    };
  }, [coordinator, themeHost]);

  // 同步系统偏好（motion / transparency）——只读、与主题协调无关。
  useEffect(() => {
    setReducedMotionState(getReducedMotion());
    setReducedTransparencyState(getReducedTransparency());
    return onPreferenceChange(() => {
      setReducedMotionState(getReducedMotion());
      setReducedTransparencyState(getReducedTransparency());
    });
  }, []);

  const setTheme = useCallback((id: string) => {
    const coord = coordinatorRef.current;
    const normalized = normalizeThemeId(id);
    if (!coord) {
      // 协调器尚未就绪（bootstrap 未完成）：DOM 仍可即时反馈（幂等），
      // Host 持久化由 bootstrap 后的 select 语义补齐。
      applyTheme(normalized);
      setSnapshot({ theme: normalized, revision: 0 });
      return;
    }
    void coord.select(normalized).catch((err) => {
      // select 失败保持旧主题（coordinator 已保证不推翻当前快照）；
      // 错误结构化可分类，供上层经 classify 展示。
      try {
        classifyThemeError(err);
      } catch {
        // 分类不可失败：兜底忽略。
      }
    });
  }, []);

  const value = useMemo<ThemeContextValue>(
    () => ({
      themeId: snapshot.theme,
      setTheme,
      isDark: snapshot.theme === 'dark',
      reducedMotion,
      reducedTransparency,
    }),
    [snapshot.theme, setTheme, reducedMotion, reducedTransparency],
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