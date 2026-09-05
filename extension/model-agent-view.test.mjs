import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { loadAgentClients } from './model-agent-controller.js';

const [view, controller, css, zh, en] = await Promise.all([
  readFile(new URL('./model-agent-view.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-agent-controller.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings.css', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/zh_CN/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/en/messages.json', import.meta.url), 'utf8'),
]);
const settingsView = await readFile(new URL('./model-settings-view.js', import.meta.url), 'utf8');
const settings = await readFile(new URL('./model-settings.js', import.meta.url), 'utf8');

console.log('--- Model Agent View Contract ---');

assert.match(settingsView, /data-page="agent"/, 'model nav must expose the agent page above advanced');
assert.match(settingsView, /data-page-panel="agent"/, 'model pages must render the agent panel');
assert.match(settingsView, /renderAgentView\(roles\.agentContainer/, 'agent view must be rendered into the agent container');
assert.match(settingsView, /data-role="agentNav"/, 'agent nav must declare its data-role for locale sync');
assert.match(settings, /handleAgentAction/, 'controller must dispatch agent actions');
assert.match(settings, /loadAgentClients\(this/, 'controller must load agent statuses when opening the page');
assert.match(view, /data-action="agent-apply"/, 'agent view must expose the apply action');
assert.match(view, /data-action="agent-default"/, 'agent view must expose the default action');
assert.match(view, /data-action="agent-close-config"/, 'agent view must expose the close action');
assert.match(view, /agent-launch/, 'agent view must expose the launch action');
assert.match(view, /data-action="agent-refresh"/, 'agent view must expose the re-detect action');
assert.match(view, /agent-tab/, 'codex sessions tab must exist in the detail view');
assert.match(view, /agent-codex-clear/, 'codex clear action must exist');
assert.match(view, /model-agent-dot/, 'client list must render status dots');
assert.match(view, /agentLaunchDirPrompt|agent-launch-prompt/, 'CLI launch must support a working directory');
assert.match(css, /\.agent-model-picker\s*\{/, 'searchable model picker styles must exist');
assert.match(css, /\.model-agent-mapping-grid\s*\{/, 'claude mapping grid styles must exist');
assert.match(css, /\.model-agent-layout\s*\{[^}]*grid-template-columns:260px minmax\(0,1fr\)/, 'agent layout must be list + detail');
assert.match(css, /\.model-agent-status-grid\s*\{[^}]*repeat\(2,/, 'status grid must be two cards side by side');
for (const key of [
  'modelAgentSettings', 'agentLocalClients', 'agentRedetect', 'agentApply', 'agentDefault',
  'agentCloseConfig', 'agentLaunch', 'agentUseModel', 'agentModifyTitle', 'agentClientVersion',
]) {
  assert.ok(JSON.parse(zh)[key] && JSON.parse(en)[key], `locale key ${key} must exist in zh_CN and en`);
}
const viewKeys = [...view.matchAll(/\bt\('([A-Za-z][\w]*)'/g)].map((match) => match[1]);
for (const key of new Set(viewKeys)) {
  assert.ok(JSON.parse(zh)[key] && JSON.parse(en)[key], `agent view locale ${key} missing`);
}

assert.doesNotMatch(controller, /await loadAgentModels\(/, 'controller must not call the removed loadAgentModels helper (stuck-loading regression)');
assert.match(settingsView, /loadError: usageContext\.agentLoadError/, 'view must forward the agent load error so failures are visible');
assert.match(settingsView, /detecting: Boolean\(usageContext\.agentDetecting\)/, 'view must forward the detecting state');
assert.match(settingsView, /activeTab: usageContext\.agentActiveTab/, 'view must forward the codex sessions tab state');
assert.match(settingsView, /sessions: usageContext\.agentSessions/, 'view must forward codex sessions');

// Functional guard: loadAgentClients must resolve, flag the detecting window,
// and always leave a recoverable state even when the host call rejects.
{
  let loading = true;
  const mkController = (listImpl) => ({
    agentStatuses: null,
    agentSelectedId: '',
    agentLoadError: '',
    agentDetecting: false,
    agentModels: null,
    agentModelsError: '',
    render: () => {},
    view: { activePage: 'agent', render: () => {} },
    api: {
      listAgentClients: listImpl,
      getAgentClientModels: async () => ({ models: [{ name: 'gemini-3.8-flash-high' }] }),
    },
  });
  const okController = mkController(async () => {
    assert.equal(okController.agentDetecting, true, 'agentDetecting must be true while detection is in flight');
    return { clients: [{ id: 'zcode', installed: true, modelPicker: true }] };
  });
  await loadAgentClients(okController, { force: true });
  assert.equal(okController.agentStatuses.length, 1, 'agent statuses must be populated on success');
  assert.equal(okController.agentSelectedId, 'zcode', 'first client must be selected by default');
  assert.deepEqual(okController.agentModels, [{ name: 'gemini-3.8-flash-high' }], 'models must load for the selected client');
  assert.equal(okController.agentDetecting, false, 'agentDetecting must reset after success');
  assert.equal(okController.agentLoadError, '', 'a successful load must clear the previous error');

  const failController = mkController(async () => { throw new Error('host offline'); });
  await loadAgentClients(failController, { force: true });
  assert.deepEqual(failController.agentStatuses, [], 'failed detection must fall back to an empty list');
  assert.match(failController.agentLoadError, /host offline/, 'failure must be recorded for the view');
  assert.equal(failController.agentDetecting, false, 'agentDetecting must reset even after failure');
}

console.log('All model agent view tests passed!');
