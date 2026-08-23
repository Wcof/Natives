// ── Usage Summary Adapter（B-024，Today Usage 与 Token Metrics 共享） ──
// 复用 useUsageData + summarizeOverviewUsage（现有查询/纯计算），
// 不直接解析日志、不建第二套 usage 状态机。
// 多个 Widget 使用相同 adapter key（usage.summary:30d:tz:project），
// 由 WorkspaceDataBroker 去重为一次查询（B-016 共享订阅）。

import { summarizeOverviewUsage } from "@/lib/personal-overview-data";
import type { WidgetConfig, WidgetDataContext } from "../types";
import { pickImperative } from "./_shared";

export interface UsageSummaryData {
  todayTokens: number | null;
  sessions: number;
  /** 近 30 天 Token 总量。 */
  totalTokens: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  messages: number;
  activeProjects: number;
  available: boolean;
}

const IMPERATIVE_CANDIDATES = [
  "loadUsageData",
  "fetchUsageData",
  "loadUsage",
  "fetchUsage",
  "queryUsage",
  "getUsageData",
];

function localDateKey(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function timeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

/** 与 Menubar/设置摘要一致的冻结视图：当前时区、30d、无项目过滤。 */
export function usageAdapterKey(_config: WidgetConfig): string {
  return `usage.summary:30d:${timeZone()}:null`;
}

interface OverviewSummaryLike {
  todayTokens?: unknown;
  sessions?: unknown;
  totalTokens?: unknown;
  inputTokens?: unknown;
  outputTokens?: unknown;
  messages?: unknown;
  activeProjects?: unknown;
}

export async function loadUsageSummary(ctx: WidgetDataContext): Promise<UsageSummaryData> {
  let usage: unknown = null;
  try {
    const mod = await import("@/hooks/useUsageData");
    const fn = pickImperative(mod, IMPERATIVE_CANDIDATES);
    if (typeof fn === "function") {
      const raw = await fn({ preset: "30d", timeZone: timeZone(), projectPath: null });
      if (ctx.signal.aborted) throw new DOMException("Aborted", "AbortError");
      usage = raw;
    }
  } catch (err) {
    if (err instanceof DOMException && err.name === "AbortError") throw err;
    console.warn("[widget] usage loader fallback (no imperative facade found):", err);
  }

  if (usage == null) {
    return {
      todayTokens: null,
      sessions: 0,
      totalTokens: null,
      inputTokens: null,
      outputTokens: null,
      messages: 0,
      activeProjects: 0,
      available: false,
    };
  }

  try {
    const summary = summarizeOverviewUsage(usage as never, localDateKey()) as unknown as OverviewSummaryLike;
    return {
      todayTokens: typeof summary.todayTokens === "number" ? summary.todayTokens : null,
      sessions: typeof summary.sessions === "number" ? summary.sessions : 0,
      totalTokens: typeof summary.totalTokens === "number" ? summary.totalTokens : null,
      inputTokens: typeof summary.inputTokens === "number" ? summary.inputTokens : null,
      outputTokens: typeof summary.outputTokens === "number" ? summary.outputTokens : null,
      messages: typeof summary.messages === "number" ? summary.messages : 0,
      activeProjects: typeof summary.activeProjects === "number" ? summary.activeProjects : 0,
      available: true,
    };
  } catch (err) {
    console.warn("[widget] usage summary normalize failed:", err);
    return {
      todayTokens: null,
      sessions: 0,
      totalTokens: null,
      inputTokens: null,
      outputTokens: null,
      messages: 0,
      activeProjects: 0,
      available: false,
    };
  }
}
