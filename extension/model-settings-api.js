import { createNativeClient } from './native-client.js';

const WRITE_METHODS = new Set([
  'model_provider_create', 'model_provider_update', 'model_provider_delete', 'model_provider_set_enabled',
	'model_provider_test',
  'model_oauth_start', 'model_oauth_cancel', 'model_account_set_enabled', 'model_account_reauth', 'model_account_delete',
  'model_models_refresh', 'model_models_upsert', 'model_models_delete', 'model_models_set_enabled',
  'model_gateway_start', 'model_gateway_stop', 'model_gateway_set_resident', 'model_gateway_rotate_access_key',
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
    setResident: (params) => client.call('model_gateway_set_resident', params),
    rotateAccessKey: (params) => client.call('model_gateway_rotate_access_key', params),
    revealAccessKey: () => client.call('model_gateway_reveal_access_key'),
    disconnect: () => client.disconnect(),
    get connected() { return client.connected; },
  };
}
