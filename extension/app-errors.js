// Shared user-facing classification for Host errors on product pages
// (plan §3.4): retired module distribution methods surface as an
// update-Natives prompt, never a retry of a removed chain.

export function classifyAppError(error) {
  const code = error?.code === 'internal_error'
    ? /APP_[A-Z_]+/.exec(error.message)?.[0] : error?.code;
  if (code === 'APP_CANCELLED') return 'appsCancelled';
  if (['APP_NETWORK', 'request_timeout'].includes(code)) return 'appsNetworkError';
  if (code === 'host_disconnected') return 'appsHostOffline';
  if (code === 'APP_HOST_UPDATE_REQUIRED' || code === 'APP_RETIRED_METHOD') return 'appsNeedsUpdate';
  // dev 链路常见：重编译 runtime 后 product-source/activation 未重密封
  // （generation 不一致或载荷 SHA 不匹配），提示重跑 dev 密封流程而非笼统重试。
  if (code === 'APP_INSTALLATION_CHANGED') return 'appsActivationChanged';
  if (code === 'APP_PACKAGE_INVALID') return 'appsPackageInvalid';
  if (['APP_BUSY', 'APP_CONFLICT'].includes(code)) return 'appsBusy';
  if (['APP_CONFIRMATION_REQUIRED', 'APP_RECOVERY_PENDING'].includes(code)) return 'appsOperationFailed';
  // 残留会话（页面切换后旧 runtime 还在 2s 自退窗口内）：明确告知原因，
  // 引导用户稍候重试，而不是笼统的"操作未完成"。
  if (['APP_RUNNING_ELSEWHERE', 'APP_ALREADY_RUNNING'].includes(code)) return 'appsRunningElsewhere';
  return 'appsOperationFailed';
}
