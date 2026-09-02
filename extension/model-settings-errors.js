const MESSAGES = {
  invalid_request: ['modelErrorInvalidRequest', '输入内容无效，请检查后重试。'], not_found: ['modelErrorNotFound', '对应配置已不存在，请刷新。'], revision_conflict: ['modelErrorRevisionConflict', '配置已在其他页面更新，请刷新后重试。'],
  keychain_unavailable: ['modelErrorKeychainUnavailable', '系统钥匙串当前不可用。'], secret_not_configured: ['modelErrorSecretMissing', '请先配置凭证。'], upstream_unreachable: ['modelErrorUpstreamUnreachable', '无法连接供应商，请检查地址和网络。'],
  upstream_unauthorized: ['modelErrorUpstreamUnauthorized', '供应商拒绝了凭证，请检查 API Key。'], upstream_error: ['modelErrorUpstream', '供应商请求失败，请稍后重试。'], upstream_invalid_response: ['modelErrorUpstreamInvalid', '供应商返回了无效响应。'],
  upstream_response_too_large: ['modelErrorUpstreamTooLarge', '供应商响应超过大小限制。'], gateway_start_failed: ['modelErrorGatewayStart', '本地模型代理启动失败。'], gateway_stop_failed: ['modelErrorGatewayStop', '本地模型代理停止失败。'],
  gateway_restart_failed: ['modelErrorGatewayRestart', '配置已保存，但代理重启失败，请重试。'], oauth_in_progress: ['modelErrorOAuthInProgress', '已有 OAuth 授权正在进行。'], internal_error: ['modelErrorInternal', '模型设置操作失败。'],
  host_disconnected: ['modelHostDisconnected', '模型 Host 已断开'], request_timeout: ['modelErrorTimeout', '请求超时，请重试。'],
};

export function localizedModelError(error, t) {
  const message = typeof error === 'object' && MESSAGES[error.code];
  return message ? t(...message) : String(error?.message || error);
}
