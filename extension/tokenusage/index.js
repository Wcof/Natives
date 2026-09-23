// tokenusage 模块核心统一入口（ADR-0028 / ADR-0030 / ADR-0031）。
//
// 集中提供 AI 计量、Token 用量与费用统计的核心抽象：
// 1. Client 层：连接管理、引用计数与事件分发（acquireModelApi, releaseModelApi, subscribeModelEvents）
// 2. Query 层：同参合并、基于 revision 缓存与自动失效（sharedCall, invalidateQueries）
// 3. Metric 层：指标定义、归一化数据抓取与格式化函数（METRICS, fetchDataset, formatTokens, formatUsd 等）
// 4. 高阶门面函数：无需手动处理资源生命周期，一站式获取 AI 计量数据

export * from './client.js';
export * from './shared-queries.js';
export * from './metrics.js';

import { acquireModelApi, releaseModelApi } from './client.js';
import { sharedCall } from './shared-queries.js';
import { fetchDataset, METRICS, DEFAULT_METRIC } from './metrics.js';

/**
 * 一站式获取 AI 计量指标数据集。
 * 内部自动处理 acquireModelApi / releaseModelApi 资源管理。
 *
 * @param {Object} options
 * @param {string|Object} [options.metric='token_usage'] - 指标 ID 或指标对象
 * @param {string} [options.range='7d'] - 时间范围 ('24h'|'72h'|'7d'|'30d'|'custom'|'all')
 * @param {Object} [options.need] - 需求字段 { overview: true, analytics: true, events: true }
 * @param {Object} [options.params] - 附加参数（如 toolIds, startTime, endTime, timezone 等）
 * @returns {Promise<Object>} 归一化数据集
 */
export async function fetchAiMetrics({
  metric = DEFAULT_METRIC,
  range = '7d',
  need = { overview: true, analytics: true, events: false },
  params = {},
} = {}) {
  const metricObj = typeof metric === 'string' ? (METRICS[metric] || METRICS[DEFAULT_METRIC]) : metric;
  const api = acquireModelApi();
  try {
    return await fetchDataset(api, metricObj, range, need, params);
  } finally {
    releaseModelApi();
  }
}

/**
 * 快速获取 AI 用量总览（Overview）。
 * 自动使用共享缓存与并发合并。
 */
export async function fetchAiOverview(params = {}) {
  const api = acquireModelApi();
  try {
    return await sharedCall(api, 'getUsageOverview', params);
  } finally {
    releaseModelApi();
  }
}

/**
 * 快速获取 AI 用量多维分析（Analysis）。
 * 自动使用共享缓存与并发合并。
 */
export async function fetchAiAnalysis(params = {}) {
  const api = acquireModelApi();
  try {
    return await sharedCall(api, 'getUsageAnalysis', params);
  } finally {
    releaseModelApi();
  }
}

/**
 * 快速获取 AI 会话聚合（Sessions）。
 * 自动使用共享缓存与并发合并。
 */
export async function fetchAiSessions(params = {}) {
  const api = acquireModelApi();
  try {
    return await sharedCall(api, 'getUsageSessions', params);
  } finally {
    releaseModelApi();
  }
}

/**
 * 快速获取 AI 原始事件流（Events）。
 */
export async function fetchAiEvents(params = {}) {
  const api = acquireModelApi();
  try {
    return await sharedCall(api, 'getUsageEvents', params);
  } finally {
    releaseModelApi();
  }
}
