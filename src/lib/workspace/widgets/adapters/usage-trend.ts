// ── Usage Trend Adapter（B-032 图表 Widget 数据源） ──
// 直接消费 Host usage facade（`@/lib/tauri/usage` 的 getCached），
// 用既有纯函数 buildDailyTrend 聚合为日趋势序列。不解析日志、不建第二套状态机。
// Adapter 固定取最宽 90d；7d/30d/90d 切换由 Widget 本地切片完成（瞬时、无重复请求）。

import { usage } from "@/lib/tauri/usage";
import { buildDailyTrend } from "@/lib/usage-dashboard";
import type { DailyTrendPoint, UsageDashboardResponse } from "@/types/usage";
import type { WidgetConfig, WidgetDataContext } from "../types";

export type UsageTrendRange = "7d" | "30d" | "90d";

export interface UsageTrendData {
  range: UsageTrendRange;
  /** 按日期升序的日趋势点（token 总量，90d 全量）。 */
  points: DailyTrendPoint[];
  /** 趋势可用性（Host 缓存缺失时 false）。 */
  available: boolean;
}

function timeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

/** 冻结 key：90d + 时区 + 无项目过滤（DataBroker 同 key 去重；范围切片在 Widget 内）。 */
export function usageTrendAdapterKey(_config: WidgetConfig): string {
  return `usage.trend:90d:${timeZone()}:null`;
}

function normalizePoints(response: UsageDashboardResponse | null): DailyTrendPoint[] {
  if (!response) return [];
  return buildDailyTrend(response.daily, response.activity);
}

export async function loadUsageTrend(
  ctx: WidgetDataContext,
): Promise<UsageTrendData> {
  try {
    const response = (await usage.getCached({
      preset: "90d",
      timeZone: timeZone(),
      projectPath: null,
    })) as unknown;
    if (ctx.signal.aborted) throw new DOMException("Aborted", "AbortError");
    if (!response || typeof response !== "object") {
      return { range: "90d", points: [], available: false };
    }
    return {
      range: "90d",
      points: normalizePoints(response as UsageDashboardResponse | null),
      available: true,
    };
  } catch (err) {
    if (err instanceof DOMException && err.name === "AbortError") throw err;
    console.warn("[widget] usage trend loader unavailable:", err);
    return { range: "90d", points: [], available: false };
  }
}
