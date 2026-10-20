import { acquireModelApi, releaseModelApi } from './ai-usage-client/client.js';

const MODEL_CAPABILITIES = new Set(['ai.usage.read', 'ai.quota.read', 'ai.quota.refresh']);

export function acquireCapabilityLeases(requires = []) {
  const capabilities = [...new Set(requires)];
  const unknown = capabilities.find((capability) => !MODEL_CAPABILITIES.has(capability));
  if (unknown) throw new Error(`unsupported required capability: ${unknown}`);
  if (!capabilities.length) return () => {};
  const api=acquireModelApi();
  let released=false;
  const release=()=>{if(!released){released=true;releaseModelApi();}};
  // Acquire the actual shared Host before a module calls its narrow query API.
  release.ready=api.getUsageStatus();
  return release;
}
