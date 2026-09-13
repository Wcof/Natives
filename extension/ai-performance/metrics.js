// Metric layer (ADR-0028 D3).
//
// A Metric turns existing model_usage_overview / model_usage_analysis /
// model_usage_events responses into one normalized dataset; charts never
// touch the protocol themselves. All data comes pre-aggregated from the Host
// (R-P discipline: no raw event tables in the renderer).

function formatTokens(value) {
  const v = Number(value) || 0;
  if (Math.abs(v) >= 1e9) return `${(v / 1e9).toFixed(1)}B`;
  if (Math.abs(v) >= 1e6) return `${(v / 1e6).toFixed(1)}M`;
  if (Math.abs(v) >= 1e3) return `${(v / 1e3).toFixed(1)}K`;
  return String(Math.round(v));
}

function formatUsd(value) {
  const v = Number(value) || 0;
  if (Math.abs(v) > 0 && Math.abs(v) < 0.01) return `$${v.toFixed(4)}`;
  return `$${v.toFixed(2)}`;
}

function formatInt(value) {
  return String(Math.round(Number(value) || 0));
}

function formatPercent(value) {
  // Host 协议单位为 0–100（store.go SuccessRate），此处只格式化不换算。
  const v = Number(value) || 0;
  return `${v.toFixed(1)}%`;
}

function formatTime(isoString) {
  if (!isoString) return '';
  try {
    const d = new Date(isoString);
    if (Number.isNaN(d.getTime())) return '';
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  } catch {
    return '';
  }
}

function toPoints(trend, key) {
  return (trend || [])
    .filter((p) => p && p.timestamp)
    .map((p) => ({ t: p.timestamp, v: Number(p[key]) || 0 }));
}

// Hourly trend points bucketed by calendar day for heatmap consumption.
function toDays(points) {
  const byDate = new Map();
  for (const p of points || []) {
    const date = String(p.t).slice(0, 10);
    if (!date) continue;
    byDate.set(date, (byDate.get(date) || 0) + p.v);
  }
  return [...byDate.entries()]
    .map(([date, v]) => ({ date, v }))
    .sort((a, b) => (a.date < b.date ? -1 : 1));
}

const RANKS = {
  model: (a) => a?.byModel || [],
  source: (a) => a?.bySource || [],
  provider: (a) => a?.byProvider || [],
  hour: (a) => a?.byHour || [],
};

function toRank(items, valueKey) {
  return (items || []).map((item) => ({
    name: item?.name || item?.key || '',
    // R7 钻取：保留 Host 维度 key（model/source/provider），点击行时用作详情筛选。
    key: item?.key || item?.name || '',
    value: Number(item?.[valueKey] ?? item?.tokens ?? item?.requests) || 0,
  }));
}

function toTableRows(items) {
  return (items || []).map((item) => ({
    name: item?.name || item?.key || '',
    key: item?.key || item?.name || '',
    requests: Number(item?.requests) || 0,
    tokens: Number(item?.tokens) || 0,
    cost: Number(item?.costUsd) || 0,
  }));
}

function toTimelineEvents(events) {
  return (events || []).map((ev) => ({
    id: ev.id || String(Math.random()),
    // Host Event JSON 字段是 requestedAt（usage/types.go）；trend 点才是 timestamp。
    time: formatTime(ev.requestedAt ?? ev.timestamp),
    source: ev.source || 'AI Client',
    model: ev.model || '—',
    tokens: Number(ev.totalTokens) || 0,
    cost: (Number(ev.costMicro) || 0) / 1e6,
    // priced=价格命中（金额可为 0）；unpriced=目录缺价，不与合法零价混淆。
    costStatus: ev.costStatus === 'unpriced' ? 'unpriced' : 'priced',
    status: ev.result === 'success' || !ev.result ? 'success' : 'error',
  }));
}

// userTimezone 返回浏览器当前 IANA 时区（Space/设置未显式传入时的默认值）；
// 取不到时返回空串（Host 端按 UTC 处理，不猜测）。
export function userTimezone() {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || '';
  } catch {
    return '';
  }
}

export const METRICS = {
  token_usage: {
    id: 'token_usage',
    name: 'Token Usage',
    dimensions: ['time', 'source', 'model'],
    numberKey: 'totalTokens',
    trendKey: 'tokens',
    rankDimension: 'model',
    rankValueKey: 'tokens',
    sessions: false,
    subMetrics: [
      { id: 'total', labelKey: 'aiPerfSubTokenTotal', fallback: '全部 Token', key: 'totalTokens' },
      { id: 'input', labelKey: 'aiPerfSubTokenInput', fallback: '输入 Token', key: 'input' },
      { id: 'output', labelKey: 'aiPerfSubTokenOutput', fallback: '输出 Token', key: 'output' },
    ],
    fmt: { value: formatTokens, rank: formatTokens },
    labelKey: 'aiPerfMetricToken',
    fallback: 'Token 用量',
  },
  ai_cost: {
    id: 'ai_cost',
    name: 'AI Cost',
    dimensions: ['time', 'model', 'provider'],
    numberKey: 'estimatedCostUsd',
    trendKey: 'costUsd',
    rankDimension: 'model',
    rankValueKey: 'costUsd',
    sessions: false,
    subMetrics: [
      { id: 'total', labelKey: 'aiPerfSubCostTotal', fallback: '预估费用', key: 'estimatedCostUsd' },
    ],
    fmt: { value: formatUsd, rank: formatUsd },
    labelKey: 'aiPerfMetricCost',
    fallback: 'AI 成本',
  },
  request_count: {
    id: 'request_count',
    name: 'Request Count',
    dimensions: ['time', 'source', 'model', 'result'],
    numberKey: 'totalRequests',
    trendKey: 'requests',
    rankDimension: 'source',
    rankValueKey: 'requests',
    sessions: false,
    subMetrics: [
      { id: 'total', labelKey: 'aiPerfSubReqTotal', fallback: '总请求数', key: 'totalRequests' },
      { id: 'rate', labelKey: 'aiPerfSubReqRate', fallback: '成功率', key: 'successRate' },
    ],
    fmt: { value: formatInt, rank: formatInt },
    labelKey: 'aiPerfMetricRequest',
    fallback: '请求数',
  },
  session_duration: {
    id: 'session_duration',
    name: 'Session Activity',
    dimensions: ['time', 'source'],
    numberKey: 'totalRequests',
    trendKey: 'requests',
    rankDimension: 'hour',
    rankValueKey: 'requests',
    sessions: false,
    subMetrics: [
      { id: 'total', labelKey: 'aiPerfSubSessionActivity', fallback: '调用频次', key: 'totalRequests' },
    ],
    fmt: { value: formatInt, rank: formatInt },
    labelKey: 'aiPerfMetricSession',
    fallback: '使用活跃度',
  },
  // R3（方案 §4.1.1/§7.4）：会话活跃消费真实 session 聚合
  // （model_usage_sessions 的 distinct (source, sessionId)），
  // 不再借 totalRequests。旧 session_duration 保留请求近似口径供兼容迁移。
  ai_sessions: {
    id: 'ai_sessions',
    name: 'AI Sessions',
    dimensions: ['time', 'source'],
    numberKey: 'totalSessions',
    trendKey: 'sessions',
    rankDimension: 'source',
    rankValueKey: 'requests',
    sessions: true,
    subMetrics: [
      { id: 'total', labelKey: 'aiPerfSubSessionCount', fallback: '活跃会话数', key: 'totalSessions' },
      { id: 'days', labelKey: 'aiPerfSubSessionDays', fallback: '活跃天数', key: 'activeDays' },
    ],
    fmt: { value: formatInt, rank: formatInt },
    labelKey: 'aiPerfMetricSessions',
    fallback: '会话活跃',
  },
};

export const DEFAULT_METRIC = 'token_usage';
export const RANK_OF = RANKS;

// `need` selects the minimal set of protocol calls for the chosen chart:
// number/line/heatmap need overview; bar/table need analytics; timeline needs events.
// R3：metric.sessions=true 时改走 model_usage_sessions（真实会话聚合），
// 不借 overview 的 totalRequests。
export async function fetchDataset(api, metric, range, need, options = {}) {
  const dimension = options.dimension || metric.rankDimension || 'model';
  const subMetric = options.subMetric || 'total';
  // toolIds 多选 → Host Filter.Sources（工具内部 ID）；空数组/未配置 = 全部已接入来源。
  // 整改 E2 §4.2：用户 IANA 时区随 filter 透传，活跃天数/日桶/趋势在
  // Model Host 内按该时区分桶（同一 filter 的所有视图共用同一时区）。
  const sources = Array.isArray(options.toolIds) && options.toolIds.length
    ? options.toolIds.filter((id) => typeof id === 'string' && id)
    : null;
  const timezone = typeof options.timezone === 'string' && options.timezone
    ? options.timezone
    : userTimezone();
  // 自定义周期：起止随查询透传（Host Filter.custom 校验 start < end）；
  // 缺失或无效时退回近 7 天缺省，保证卡片始终可查询。
  const extraRange = {};
  if (range === 'custom') {
    const start = Date.parse(options.startTime);
    const end = Date.parse(options.endTime);
    if (Number.isFinite(start) && Number.isFinite(end) && start < end) {
      extraRange.startTime = new Date(start).toISOString();
      extraRange.endTime = new Date(end).toISOString();
    } else {
      const endMs = Date.now();
      extraRange.startTime = new Date(endMs - 7 * 86400000).toISOString();
      extraRange.endTime = new Date(endMs).toISOString();
    }
  }
  const scopeParams = {
    range,
    ...extraRange,
    ...(sources ? { sources } : {}),
    ...(timezone ? { timezone } : {}),
  };

  if (metric.sessions) {
    const sessions = await api.getUsageSessions({ ...scopeParams });
    if (!sessions || sessions.status === 'empty') return { empty: true };
    const sub = subMetric === 'days' ? 'activeDays' : 'totalSessions';
    const trend = (sessions.byDay || []).map((d) => ({ t: d.date, v: Number(d.sessions) || 0 }));
    const rank = (sessions.bySource || []).map((it) => ({
      name: it.name || it.key || '',
      value: Number(it.requests) || 0,
    }));
    const tableRows = (sessions.bySource || []).map((it) => ({
      name: it.name || it.key || '',
      requests: Number(it.requests) || 0,
      tokens: Number(it.tokens) || 0,
      cost: Number(it.costUsd) || 0,
    }));
    return {
      empty: false,
      number: {
        value: Number(sessions[sub]) || 0,
        isPercent: false,
        subLabel: sessions.unattributed > 0
          ? `${formatInt(sessions.unattributed)} 条未归属请求`
          : '',
      },
      trend,
      days: toDays(trend),
      rank,
      tableRows,
      events: [],
    };
  }

  const [overview, analytics, eventsRes] = await Promise.all([
    need.overview ? api.getUsageOverview({ ...scopeParams }) : null,
    need.analytics ? api.getUsageAnalysis({ ...scopeParams }) : null,
    need.events ? api.getUsageEvents({ ...scopeParams, limit: options.limit || 8 }) : null,
  ]);

  const isOverviewEmpty = Boolean(need.overview && (!overview || overview.status === 'empty'));
  const isAnalyticsEmpty = Boolean(
    need.analytics &&
      (!analytics ||
        analytics.status === 'empty' ||
        (!analytics.byModel?.length &&
          !analytics.bySource?.length &&
          !analytics.byProvider?.length &&
          !analytics.byHour?.length))
  );
  const isEventsEmpty = Boolean(
    need.events && (!eventsRes || !eventsRes.events || eventsRes.events.length === 0)
  );

  if (isOverviewEmpty || isAnalyticsEmpty || isEventsEmpty) {
    return { empty: true };
  }

  // Calculate primary display value according to subMetric
  let numValue = Number(overview?.metrics?.[metric.numberKey]) || 0;
  let subLabel = '';
  if (metric.id === 'token_usage') {
    if (subMetric === 'input') {
      numValue = Number(overview?.tokens?.input) || 0;
      subLabel = 'Input Tokens';
    } else if (subMetric === 'output') {
      numValue = Number(overview?.tokens?.output) || 0;
      subLabel = 'Output Tokens';
    } else {
      subLabel = `In: ${formatTokens(overview?.tokens?.input || 0)} / Out: ${formatTokens(overview?.tokens?.output || 0)}`;
    }
  } else if (metric.id === 'request_count') {
    if (subMetric === 'rate') {
      numValue = Number(overview?.metrics?.successRate) || 0;
      subLabel = '成功率';
    } else {
      const rate = Number(overview?.metrics?.successRate);
      if (!Number.isNaN(rate) && rate > 0) {
        subLabel = `成功率: ${formatPercent(rate)}`;
      }
    }
  }

  const trend = toPoints(overview?.trend, metric.trendKey);
  const rankItems = RANKS[dimension]?.(analytics) || RANKS[metric.rankDimension]?.(analytics) || [];

  return {
    empty: false,
    number: {
      value: numValue,
      isPercent: subMetric === 'rate',
      subLabel,
    },
    trend,
    days: toDays(trend),
    rank: analytics ? toRank(rankItems, metric.rankValueKey) : [],
    tableRows: analytics ? toTableRows(rankItems) : [],
    events: eventsRes ? toTimelineEvents(eventsRes.events) : [],
  };
}
