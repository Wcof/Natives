// Preview 错误分类（T10 · C0 frozen）
//
// fallback 必须显式：仅 recoverable 可继续尝试下一 provider；
// fatal（权限/安全/IO/Host）立即停止并显式呈现，禁止 catch-all 掩盖。
// AbortError/cancelled 静默终止当前 Surface request，不产生日志噪声。

export type PreviewRecoverableCode = 'not_applicable' | 'unsupported' | 'parse_failed';

export type PreviewFatalCode =
  | 'permission_denied'
  | 'security_violation'
  | 'io_error'
  | 'host_error'
  | 'cancelled';

export type PreviewErrorCode = PreviewRecoverableCode | PreviewFatalCode;

const RECOVERABLE = new Set<PreviewErrorCode>(['not_applicable', 'unsupported', 'parse_failed']);

export class PreviewProviderError extends Error {
  readonly code: PreviewErrorCode;
  readonly recoverable: boolean;

  constructor(code: PreviewErrorCode, message: string) {
    super(message);
    this.name = 'PreviewProviderError';
    this.code = code;
    this.recoverable = RECOVERABLE.has(code);
  }
}

/** 构造可降级错误（provider 不适用/不支持/解析失败 → 尝试下一 provider） */
export function recoverableError(code: PreviewRecoverableCode, message: string): PreviewProviderError {
  return new PreviewProviderError(code, message);
}

/** 构造致命错误（权限/安全/IO/Host → 停止并显式呈现，不 fallback） */
export function fatalError(code: PreviewFatalCode, message: string): PreviewProviderError {
  return new PreviewProviderError(code, message);
}

/** 请求被 Surface 取消：静默终止，不算 provider 失败 */
export function cancelledError(message = 'request cancelled'): PreviewProviderError {
  return new PreviewProviderError('cancelled', message);
}
