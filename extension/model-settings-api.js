import { createNativeClient } from './native-client.js';

const WRITE_METHODS = new Set([
  'model_provider_create', 'model_provider_update', 'model_provider_delete', 'model_provider_set_enabled',
  'model_provider_test',
  'model_oauth_start', 'model_oauth_cancel', 'model_account_set_enabled', 'model_account_reauth', 'model_account_delete',
  'model_models_refresh', 'model_models_upsert', 'model_models_delete', 'model_models_set_enabled',
  'model_gateway_start', 'model_gateway_stop', 'model_gateway_restart', 'model_gateway_set_resident',
  'model_gateway_settings_update', 'model_gateway_key_create', 'model_gateway_key_update', 'model_gateway_key_delete',
  'model_gateway_key_rotate', 'model_gateway_rotate_access_key',
  'model_usage_price_upsert', 'model_usage_price_delete', 'model_usage_price_sync',
  'model_usage_import_begin', 'model_usage_import_chunk', 'model_usage_import_commit', 'model_usage_import_cancel',
  'model_account_models_update',
]);

export function createModelSettingsAPI({ onEvent, onDisconnect } = {}) {
  const client = createNativeClient({
    host: 'com.natives.model_host',
    writeMethods: WRITE_METHODS,
    timeoutMs: 30_000,
    onEvent,
    onDisconnect,
  });

  return {
    snapshot: () => client.call('model_snapshot'),
    loadSnapshot: () => client.call('model_snapshot'),
    createProvider: (params) => client.call('model_provider_create', params),
    updateProvider: (params) => client.call('model_provider_update', params),
    deleteProvider: (params) => client.call('model_provider_delete', params),
    setProviderEnabled: (params) => client.call('model_provider_set_enabled', params),
    testProvider: (params) => client.call('model_provider_test', params),
    startOAuth: (params) => client.call('model_oauth_start', params),
    cancelOAuth: (params) => client.call('model_oauth_cancel', params),
    setAccountEnabled: (params) => client.call('model_account_set_enabled', params),
    reauthAccount: (params) => client.call('model_account_reauth', params),
    deleteAccount: (params) => client.call('model_account_delete', params),
    refreshModels: (params) => client.call('model_models_refresh', params),
    upsertModel: (params) => client.call('model_models_upsert', params),
    deleteModel: (params) => client.call('model_models_delete', params),
    setModelEnabled: (params) => client.call('model_models_set_enabled', params),
    startGateway: (params) => client.call('model_gateway_start', params),
    stopGateway: (params) => client.call('model_gateway_stop', params),
    restartGateway: (params) => client.call('model_gateway_restart', params),
    setResident: (params) => client.call('model_gateway_set_resident', params),
    updateGatewaySettings: (params) => client.call('model_gateway_settings_update', params),
    createGatewayKey: (params) => client.call('model_gateway_key_create', params),
    updateGatewayKey: (params) => client.call('model_gateway_key_update', params),
    deleteGatewayKey: (params) => client.call('model_gateway_key_delete', params),
    rotateGatewayKey: (params) => client.call('model_gateway_key_rotate', params),
    revealGatewayKey: (params) => client.call('model_gateway_key_reveal', params),
    rotateAccessKey: (params) => client.call('model_gateway_rotate_access_key', params),
    revealAccessKey: () => client.call('model_gateway_reveal_access_key'),
    checkKernelUpdate: () => client.call('model_kernel_check_update'),
    updateKernel: () => client.call('model_kernel_update'),

    
    getUsageStatus: () => client.call('model_usage_status'),
    getUsageOverview: (params) => client.call('model_usage_overview', params),
    getUsageAnalysis: (params) => client.call('model_usage_analysis', params),
    getUsageEvents: (params) => client.call('model_usage_events', params),
    getUsagePricing: () => client.call('model_usage_pricing'),
    upsertUsagePrice: (params) => client.call('model_usage_price_upsert', params),
    deleteUsagePrice: (params) => client.call('model_usage_price_delete', params),
    syncUsagePrice: () => client.call('model_usage_price_sync'),
    beginUsageImport: (params) => client.call('model_usage_import_begin', params),
    chunkUsageImport: (params) => client.call('model_usage_import_chunk', params),
    previewUsageImport: (params) => client.call('model_usage_import_preview', params),
    commitUsageImport: (params) => client.call('model_usage_import_commit', params),
    cancelUsageImport: (params) => client.call('model_usage_import_cancel', params),

    
    listAuthFiles: () => client.call('model_auth_files_list'),
    importAuthFile: (params) => client.call('model_auth_files_import', params),
    updateAuthFile: (params) => client.call('model_auth_files_update', params),
    deleteAuthFile: (params) => client.call('model_auth_files_delete', params),
    openAuthDir: () => client.call('model_auth_files_open_dir'),
    queryQuota: (params) => client.call('model_quota_query', params),
    getAccountModels: (params) => client.call('model_account_models', params),
    updateAccountModels: (params) => client.call('model_account_models_update', params),

    
    listAgentClients: () => client.call('model_agent_clients_list'),
    getAgentClientModels: (params) => client.call('model_agent_client_models', params),
    applyAgentClientConfig: (params) => client.call('model_agent_client_apply', params),
    defaultAgentClientConfig: (params) => client.call('model_agent_client_default', params),
    closeAgentClientConfig: (params) => client.call('model_agent_client_close', params),
    launchAgentClient: (params) => client.call('model_agent_client_launch', params),
    clearCodexConfig: () => client.call('model_agent_codex_clear'),
    listCodexSessions: () => client.call('model_agent_codex_sessions_list'),
    deleteCodexSessions: (params) => client.call('model_agent_codex_sessions_delete', params),
    piProviderAction: (params) => client.call('model_agent_pi_action', params),

    disconnect: () => client.disconnect(),
    get connected() { return client.connected; },
  };
}
