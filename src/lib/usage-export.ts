// ── Usage Dashboard Export Utilities ──
// CSV 导出、SVG Badge、Markdown 摘要
// 纯函数，无 UI 依赖，可测试

import type {
  UsageDailyRecord,
  UsageActivityBucket,
  UsageSessionRecord,
  UsageMetrics,
  UsageSourceStatus,
} from '@/types/usage';

// ── CSV Export ──

/** 转义 CSV 单元格值 */
function escapeCsv(val: string): string {
  if (val.includes(',') || val.includes('"') || val.includes('\n') || val.includes('\r')) {
    return `"${val.replace(/"/g, '""')}"`;
  }
  return val;
}

/** 将 daily 记录序列化为 CSV（UTF-8 BOM） */
export function serializeUsageCsv(daily: UsageDailyRecord[]): string {
  const header = [
    'date', 'source', 'model', 'project', 'terminal',
    'input', 'output', 'cache_creation', 'cache_read', 'total',
    'cost', 'cost_quality',
  ];

  const rows = daily.map((r) => [
    r.date,
    r.sourceId,
    r.modelId ?? '',
    r.projectId ?? '',
    r.terminalId ?? '',
    r.inputTokens?.toString() ?? '',
    r.outputTokens?.toString() ?? '',
    r.cacheCreationTokens?.toString() ?? '',
    r.cacheReadTokens?.toString() ?? '',
    r.totalTokens?.toString() ?? '',
    r.costUsd?.toString() ?? '',
    r.costQuality,
  ].map((v) => escapeCsv(v)).join(','));

  return '\uFEFF' + [header.join(','), ...rows].join('\n');
}

// ── SVG Badge ──

export interface BadgeConfig {
  period: string;           // e.g. "2026-07-01 – 2026-07-30"
  tokens: number | null;    // 总 Token
  cost: number | null;      // 估算费用
  sessions: number | null;  // 会话数
}

/** 生成纯 SVG Badge（黑白灰，无外部字体/远程资源） */
export function serializeUsageBadgeSvg(config: BadgeConfig): string {
  const { period, tokens, cost, sessions } = config;

  // 只显示可用的指标
  const lines: string[] = [
    `<text x="12" y="18" font-family="monospace, sans-serif" font-size="11" fill="#f5f5f5" font-weight="700">Vibe Usage</text>`,
    `<text x="12" y="32" font-family="monospace, sans-serif" font-size="9" fill="#a3a3a3">${escapeXml(period)}</text>`,
  ];

  let yPos = 50;
  if (tokens !== null) {
    lines.push(`<text x="12" y="${yPos}" font-family="monospace, sans-serif" font-size="10" fill="#f5f5f5">Tokens: ${tokens.toLocaleString()}</text>`);
    yPos += 16;
  }
  if (cost !== null) {
    lines.push(`<text x="12" y="${yPos}" font-family="monospace, sans-serif" font-size="10" fill="#f5f5f5">Cost: $${cost.toFixed(4)}</text>`);
    yPos += 16;
  }
  if (sessions !== null) {
    lines.push(`<text x="12" y="${yPos}" font-family="monospace, sans-serif" font-size="10" fill="#f5f5f5">Sessions: ${sessions}</text>`);
    yPos += 16;
  }

  const height = Math.max(yPos + 12, 60);

  return `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="${height}" viewBox="0 0 240 ${height}">
  <rect width="240" height="${height}" rx="6" fill="#151515"/>
  ${lines.join('\n  ')}
</svg>`;
}

function escapeXml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// ── Markdown Summary ──

/** 生成 Markdown 用量摘要（不含项目名、终端名、路径、breadcrumb） */
export function serializeUsageMarkdown(
  period: string,
  metrics: UsageMetrics | null,
  totalSessions: number,
): string {
  const lines: string[] = [];
  lines.push(`## Vibe Usage · ${period}`);
  lines.push('');

  if (!metrics) {
    lines.push('_No usage data available._');
    return lines.join('\n');
  }

  if (metrics.estimatedCost !== null) {
    lines.push(`- **Est. Cost**: $${metrics.estimatedCost.toFixed(4)}`);
  }
  if (metrics.totalTokens !== null) {
    lines.push(`- **Total Tokens**: ${metrics.totalTokens.toLocaleString()}`);
  }
  if (metrics.totalInputTokens !== null) {
    lines.push(`- **Input Tokens**: ${metrics.totalInputTokens.toLocaleString()}`);
  }
  if (metrics.totalOutputTokens !== null) {
    lines.push(`- **Output Tokens**: ${metrics.totalOutputTokens.toLocaleString()}`);
  }
  if (metrics.totalCacheReadTokens !== null) {
    lines.push(`- **Cache Read**: ${metrics.totalCacheReadTokens.toLocaleString()}`);
  }
  if (metrics.totalCacheCreationTokens !== null) {
    lines.push(`- **Cache Creation**: ${metrics.totalCacheCreationTokens.toLocaleString()}`);
  }
  lines.push(`- **Sessions**: ${totalSessions}`);
  lines.push(`- **Messages**: ${metrics.totalUserMessages} user / ${metrics.totalAssistantMessages} assistant`);
  if (metrics.estimatedActiveSeconds !== null) {
    const h = Math.floor(metrics.estimatedActiveSeconds / 3600);
    const m = Math.floor((metrics.estimatedActiveSeconds % 3600) / 60);
    lines.push(`- **Active Duration**: ${h}h ${m}m`);
  }

  return lines.join('\n');
}

// ── 导出文件默认名 ──

/** 生成默认文件名 */
export function defaultExportFilename(prefix: string, date: Date = new Date()): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${prefix}-${y}${m}${d}`;
}

/** 检查是否有可分享的指标 */
export function hasShareableMetrics(metrics: UsageMetrics | null): boolean {
  if (!metrics) return false;
  return (
    metrics.estimatedCost !== null ||
    metrics.totalTokens !== null ||
    metrics.totalSessions > 0
  );
}