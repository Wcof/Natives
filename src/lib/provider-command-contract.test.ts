import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

// ARCH-002: provider commands live in the domain facade `./tauri/provider.ts`,
// not the barrel `./tauri-adapter.ts`. The contract test scans the facade
// (the single place the provider domain may issue these commands).
const adapter = readFileSync(new URL('./tauri/provider.ts', import.meta.url), 'utf8');
const dialog = readFileSync(new URL('../components/settings/AddProviderDialog.tsx', import.meta.url), 'utf8');
const settings = readFileSync(new URL('../components/shell/SettingsPage.tsx', import.meta.url), 'utf8');
const providerDir = fileURLToPath(new URL('../../src-tauri/src/commands/provider/', import.meta.url));
const commands = readdirSync(providerDir, { recursive: true })
  .filter((f): f is string => typeof f === 'string' && f.endsWith('.rs'))
  .map((f) => readFileSync(join(providerDir, f), 'utf8'))
  .join('\n');
const commandRegistry = readFileSync(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');
// T202/T302: legacy daemon/rpc_server.rs was physically deleted (orphan, 0 refs).
// A2-03 split the Agent Daemon RPC authority into rpc/{dispatch,server,handlers}.
// Read the whole rpc/ tree so the contract stays authoritative under the split.
const rpcDir = fileURLToPath(new URL('../../src-agent-daemon/src/rpc/', import.meta.url));
const agentDaemonRpc = readdirSync(rpcDir, { recursive: true })
  .filter((f): f is string => typeof f === 'string' && f.endsWith('.rs'))
  .map((f) => readFileSync(join(rpcDir, f), 'utf8'))
  .join('\n');

describe('provider renderer/Tauri contract', () => {
  it('wraps struct command arguments in input', () => {
    assert.match(adapter, /add_provider', \{ input \}/);
    assert.match(adapter, /add_provider_key', \{ input \}/);
    assert.match(adapter, /delete_provider', \{ providerId \}/);
  });

  it('only invokes registered provider commands', () => {
    for (const command of ['provider_discover_models', 'provider_update_defaults', 'provider_test', 'provider_set_primary_key', 'delete_provider_key']) {
      assert.match(commands, new RegExp(`fn ${command}\\b`));
      assert.match(commandRegistry, new RegExp(`commands::provider::${command}\\b`));
    }
  });

  it('persists the model selected in the add dialog', () => {
    assert.match(dialog, /defaultModel,/);
    assert.match(settings, /defaultModel: data\.defaultModel/);
  });

  it('persists the selected provider protocol separately from the preset', () => {
    assert.match(dialog, /apiProtocol: selectedProtocol/);
    assert.match(settings, /apiProtocol: data\.apiProtocol/);
    assert.match(commands, /api_protocol/);
  });

  it('daemon rpc implements provider methods and stays clean', () => {
    // T202/T302: the orphan host rpc_server.rs is gone; the Agent Daemon is the
    // single RPC authority. Its response text must not regress into "missing
    // content" placeholder shapes.
    assert.doesNotMatch(agentDaemonRpc, new RegExp(['Model response', 'missing content'].join(' ')));
    assert.match(agentDaemonRpc, /is_implemented_method/);
    assert.match(agentDaemonRpc, /conversation\.getContextUsage|mcp\.list/);
  });
});
