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

/**
 * lifecycle:heartbeat 专用门：来源必须是期望窗口引用，且 data 声明为
 * 该 module 的心跳。source 不匹配时即使 data.type/moduleId 正确也被拒绝
 * —— 同 origin / 伪 moduleId 的伪造消息无法伪造真实 contentWindow 引用。
 */
export function isHeartbeatFromModuleSource(
  event: MessageEventLike,
  expectedSource: unknown,
  moduleId: string,
): boolean {
  if (!isExpectedMessageSource(event, expectedSource)) return false;
  const data = event.data as { type?: unknown; moduleId?: unknown } | null;
  return (
    data !== null &&
    typeof data === 'object' &&
    data.type === 'lifecycle:heartbeat' &&
    data.moduleId === moduleId
  );
}
