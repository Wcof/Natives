'use client';

/**
 * appearance/coordinator — 主题外观单一协调权威（TH-02）。
 *
 * DOM（html[data-theme] + CSS 变量）在本层只是「输出 sink」，不是反向权威：
 * 语义真值是 Host（get_theme / set_theme），DOM 变更一律经 coordinator 单向
 * 写入；订阅者只从 coordinator 读快照。这替换了旧 ThemeContext 的
 * MutationObserver「反向 authority」模式。
 *
 * 职责边界：
 *   · bootstrap()  —— 启动读取 Host 主题 → Zod 校验 → applyTheme → ready 信号；
 *                     失败用受控 dark fallback 显窗，错误结构化可分类。
 *   · select()    —— 用户切换（set_theme），Host 持久化成功才改 DOM + 通知；
 *                     失败保持旧主题并抛出结构化分类错误。
 *   · subscribe() —— 变更监听 + Host db-state-changed「theme」频道（多窗口一致）。
 *   · getSnapshot() —— 当前 { theme, revision }。
 *
 * 约束（规范）：
 *   · DOM 只是 sink，不反向真值（product R-F2）。
 *   · 错误经 classifyError 结构化，不 console-only、不抛裸异常（R-F5 / R-E12）。
 *   · 首帧 dark fallback 与 theme-engine 兜底一致（R-U3 / FOUC guard 语义）。
 */

import { z } from 'zod';
import type { V2ThemeId } from '@/lib/design-tokens';
import { applyTheme, normalizeThemeId } from '@/lib/theme-engine';
import { classifyError } from '@/lib/error-classifier';
import type { ClassifiedError } from '@/lib/error-classifier';

// ── 类型 ────────────────────────────────────────────────────────────────

export type ThemeId = V2ThemeId;

export interface AppearanceSnapshot {
  theme: ThemeId;
  /** 单调修订号：每次应用（bootstrap / select / 广播）自增。供协商期去重重渲染。 */
  revision: number;
}

/** Host 侧主题命令契约（R-T5: get_theme / set_theme 已注册）。 */
export interface ThemeHost {
  getTheme: () => Promise<string>;
  setTheme: (theme: string) => Promise<void>;
  /** FOUC guard：CSS 变量就绪后显窗。失败容忍（浏览器 dev 无 Tauri 窗口）。 */
  themeReady: () => void;
  /** 订阅 Host db-state-changed 的 theme 频道（R-S9；多窗口一致）。 */
  onThemeChanged: (callback: (theme: ThemeId) => void) => () => void;
}

export interface AppearanceCoordinator {
  bootstrap(): Promise<AppearanceSnapshot>;
  select(theme: ThemeId): Promise<AppearanceSnapshot>;
  subscribe(listener: (snapshot: AppearanceSnapshot) => void): () => void;
  getSnapshot(): AppearanceSnapshot;
}

export interface AppearanceCoordinatorOptions {
  /**
   * 手动接管 ready 信号派发（测试用；缺省调用 host.themeReady）。
   * 不应抛未捕获异常（bootstrap 内部会容忍失败）。
   */
  signalReady?: () => void;
}

// ── 结构化分类错误 ────────────────────────────────────────────────────────

export interface ThemeErrorPayload {
  kind: 'load' | 'persist' | 'sync';
  classified: ClassifiedError;
  /** 错误发生时的当前快照（加载失败 = dark fallback；持久化失败 = 旧主题）。 */
  current: AppearanceSnapshot;
  retryable: boolean;
}

/** 主题协调错误：携带可分类的 payload（不吞、不裸抛给 UI）。 */
export class ThemeCoordinatorError extends Error {
  readonly payload: ThemeErrorPayload;

  constructor(payload: ThemeErrorPayload) {
    super(payload.classified.userMessage);
    this.name = 'ThemeCoordinatorError';
    this.payload = payload;
  }
}

/** 统一分类入口（保持 facade 轻量，测试用）。 */
export function classifyThemeError(error: unknown, locale?: string): ClassifiedError {
  return classifyError(error, { locale });
}

// ── Zod 校验（R-U3）：任何跨 IPC 进入的主题值 / 广播负载先过防线 ─────────────

const ThemeIdSchema = z
  .union([z.literal('dark'), z.literal('light')])
  .catch('dark');

/**
 * 从任意输入串解析合法主题 id：legacy 别名先经 normalize（terminal-volt→dark、
 * frosted-jasmine→light），经 Zod 防线兜底到 dark（R-U3）。
 * Host 返回值与 db-state-changed 广播载荷都经此（防御未知取值 / 未来值）。
 */
export function parseThemeId(input: string | null | undefined): ThemeId {
  const normalized = normalizeThemeId(input);
  const result = ThemeIdSchema.safeParse(normalized);
  if (result.success) return result.data;
  return 'dark';
}

// ── 生产 Host（tauri facade 收口） ──────────────────────────────────────

/**
 * 生产默认 Host：全部走 tauri facade（R-E9 唯一 raw invoke）。
 * 测试不执行本工厂，直接注入替身。
 */
export async function resolveThemeHost(base?: Partial<ThemeHost>): Promise<ThemeHost> {
  const adapter = await import('@/lib/tauri-adapter');
  const host: ThemeHost = {
    getTheme: () => adapter.getTheme(),
    setTheme: (theme) => adapter.setTheme(theme),
    themeReady: () => adapter.themeReady(),
    onThemeChanged: (callback) =>
      adapter.onDbStateChanged((_event, channel, data) => {
        if (channel !== 'theme') return;
        let next: unknown;
        if (data && typeof data === 'object' && 'theme' in (data as object)) {
          next = (data as { theme: unknown }).theme;
        } else {
          next = data;
        }
        callback(parseThemeId(typeof next === 'string' ? next : undefined));
      }),
  };
  return { ...host, ...base };
}

// ── 实现 ────────────────────────────────────────────────────────────────

const DEFAULT_THEME: ThemeId = 'dark';

/** DOM sink：唯一应用出口（applyTheme 封装，SSR/测试无 document 时容忍）。 */
function applyToDom(theme: ThemeId): void {
  if (typeof document === 'undefined') return;
  applyTheme(theme);
}

function makeSnapshot(theme: ThemeId, revision: number): AppearanceSnapshot {
  return Object.freeze({ theme, revision });
}

const DEFAULT_SNAPSHOT: AppearanceSnapshot = makeSnapshot(DEFAULT_THEME, 0);

class AppearanceCoordinatorImpl implements AppearanceCoordinator {
  private host: ThemeHost;
  private signalReady: () => void;
  private ready = false;
  private current: AppearanceSnapshot = DEFAULT_SNAPSHOT;
  private listeners = new Set<(snapshot: AppearanceSnapshot) => void>();
  private broadcastUnsub: (() => void) | null = null;
  /** 本地 select 刚提交的主题：广播回显（Host 多窗口确认）时忽略，避免重复 revision。 */
  private lastCommittedTheme: ThemeId | null = null;

  constructor(host: ThemeHost, options: AppearanceCoordinatorOptions = {}) {
    this.host = host;
    this.signalReady = options.signalReady ?? (() => this.host.themeReady());
  }

  getSnapshot(): AppearanceSnapshot {
    return this.current;
  }

  subscribe(listener: (snapshot: AppearanceSnapshot) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private notify(snapshot: AppearanceSnapshot): void {
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // 订阅者错误不得中断后续（隔离）。
      }
    }
  }

  private commit(theme: ThemeId): AppearanceSnapshot {
    const next: AppearanceSnapshot = makeSnapshot(theme, this.current.revision + 1);
    applyToDom(theme);
    this.current = next;
    this.notify(next);
    return next;
  }

  private markReady(): void {
    if (this.ready) return;
    this.ready = true;
    try {
      this.signalReady();
    } catch {
      // 浏览器 dev 无 Tauri 窗口：信号失败可容忍（R-S10 只关乎 window.show）。
    }
  }

  private connectBroadcast(): void {
    if (this.broadcastUnsub) return;
    this.broadcastUnsub = this.host.onThemeChanged((incoming) => {
      const next = parseThemeId(incoming);
      if (next === this.current.theme) return;
      // 本地 select 提交后 Host 广播回显不再重复通知（防 revision 抖动）。
      if (next === this.lastCommittedTheme) return;
      this.commit(next);
    });
  }

  async bootstrap(): Promise<AppearanceSnapshot> {
    let theme = DEFAULT_THEME;
    try {
      const raw = await this.host.getTheme();
      theme = parseThemeId(raw);
    } catch (error) {
      // 受控 dark fallback（R-F2：用可追溯兜底而非编值）：
      // 调用方（RootClient/Shell）可经 classify 展示分类重试错误。
      throw new ThemeCoordinatorError({
        kind: 'load',
        classified: classifyThemeError(error),
        current: this.getSnapshot(),
        retryable: true,
      });
    }

    this.commit(theme);
    // 先接广播再 ready：避免遗漏竞态窗口期间其它窗口的变更。
    this.connectBroadcast();
    this.markReady();
    return this.current;
  }

  async select(theme: ThemeId): Promise<AppearanceSnapshot> {
    const resolved = parseThemeId(theme);
    if (resolved === this.current.theme) {
      // 与当前一致：空操作（避免 revision 抖动）。
      return this.current;
    }
    // 先标 pending：Host set_theme 成功前，广播回显（同主题）要忽略，
    // select 只在最后 committed 通知一次订阅者。
    this.lastCommittedTheme = resolved;
    try {
      await this.host.setTheme(resolved);
    } catch (error) {
      // 持久化失败：保持旧主题 + 结构化分类错误；清掉 pending 标记。
      this.lastCommittedTheme = null;
      throw new ThemeCoordinatorError({
        kind: 'persist',
        classified: classifyThemeError(error),
        current: this.getSnapshot(),
        retryable: true,
      });
    }
    // Host 持久化成功后才更新 DOM / CSS 变量 / 通知订阅者。
    this.commit(resolved);
    return this.current;
  }
}

// ── 表面单例工厂 ─────────────────────────────────────────────────────────

/**
 * 创建协调器实例（每个 surface 一个语义：main / menubar）。
 * 纯 JS 可测：不依赖 React；DOM sink 经 applyTheme 封装。
 */
export function createAppearanceCoordinator(
  host: ThemeHost,
  options: AppearanceCoordinatorOptions = {},
): AppearanceCoordinator {
  return new AppearanceCoordinatorImpl(host, options);
}

let singleton: AppearanceCoordinator | null = null;

/**
 * 全局协调器（同进程一个真值；main + menubar 共用）。
 * 异步解析 Host facade（R-U4 依赖 tauri-adapter；测试注入 base 替身）。
 */
export async function getAppearanceCoordinator(
  base?: Partial<ThemeHost>,
): Promise<AppearanceCoordinator> {
  if (singleton) return singleton;
  const host = await resolveThemeHost(base);
  singleton = createAppearanceCoordinator(host);
  return singleton;
}

// ── 领域常量 / 兼容导出 ──────────────────────────────────────────────────

/** db-state-changed theme 频道名（与 src-tauri/commands/theme.rs 一致）。 */
export const THEME_CHANNEL = 'theme';

export const DEFAULT_APPEARANCE_THEME: ThemeId = DEFAULT_THEME;

/** 兼容别名：normalizeThemeId（旧 ThemeContext 用户读取 DOM，现统一经此解析）。 */
export { normalizeThemeId as resolveTheme, applyTheme };