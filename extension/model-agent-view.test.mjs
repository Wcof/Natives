import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

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
assert.match(view, /data-action="agent-launch"/, 'agent view must expose the launch action');
assert.match(view, /data-action="agent-refresh"/, 'agent view must expose the re-detect action');
assert.match(view, /model-agent-dot/, 'client list must render status dots');
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

console.log('All model agent view tests passed!');
