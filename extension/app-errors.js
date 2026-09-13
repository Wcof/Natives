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
  if (['APP_BUSY', 'APP_CONFLICT'].includes(code)) return 'appsBusy';
  if (['APP_CONFIRMATION_REQUIRED', 'APP_RECOVERY_PENDING'].includes(code)) return 'appsOperationFailed';
  return 'appsOperationFailed';
}
