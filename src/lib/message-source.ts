/**
 * postMessage 来源校验统一 helper（R-S3）。
 *
 * R-S3（MUST）：基座验证 postMessage 来源时必须用 MessageEvent.source 窗口
 * 引用匹配（创建 iframe 时保存 contentWindow，收消息时比对），禁止依赖
 * event.origin —— sandbox iframe 的 origin 恒为 "null"，无法区分真实发送者。
 *
 * 所有接收宿主侧 window 'message' 事件的处理链都应复用本模块，避免每个
 * hook / manager 自写一套来源校验。
 */

/** 最小 MessageEvent 结构；与 DOM 的 MessageEvent 结构兼容，便于纯函数测试。 */
export interface MessageEventLike {
  source: unknown;
  data?: unknown;
}

/**
 * 窄来源校验：消息必须来自期望的窗口引用（iframe.contentWindow / window.parent）。
 * event.source 为 null / undefined（同进程合成消息等）一律拒绝。
 */
export function isExpectedMessageSource(
  event: MessageEventLike,
  expectedSource: unknown,
): boolean {
  return (
    event.source !== null &&
    event.source !== undefined &&
    event.source === expectedSource
  );
}

/** Workshop lifecycle messages require the exact iframe source and the
 * current module-bound session token. `origin` is intentionally irrelevant
 * because a sandbox without allow-same-origin has an opaque origin. */
export function isAuthenticatedLifecycleMessage(
  event: MessageEventLike,
  expectedSource: unknown,
  moduleId: string,
  currentToken: string | undefined,
): boolean {
  if (!currentToken || !isExpectedMessageSource(event, expectedSource)) return false;
  const data = event.data as {
    type?: unknown;
    moduleId?: unknown;
    token?: unknown;
  } | null;
  return (
    data !== null &&
    typeof data === 'object' &&
    (data.type === 'lifecycle:ready' ||
      data.type === 'lifecycle:heartbeat' ||
      data.type === 'lifecycle:error') &&
    data.moduleId === moduleId &&
    data.token === currentToken
  );
}
