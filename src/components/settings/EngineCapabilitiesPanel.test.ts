import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';

test('EngineCapabilitiesPanel uses createDefaultGateway + capability-admin only', () => {
  const src = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/EngineCapabilitiesPanel.tsx'),
    'utf8',
  );
  assert.match(src, /createDefaultGateway/);
  assert.match(src, /listCapabilitySkills/);
  assert.match(src, /listCapabilityMcpServers/);
  assert.match(src, /listCapabilityExperts/);
  assert.match(src, /listCapabilityTeams/);
  assert.match(src, /listExtensions/);
  assert.match(src, /getRateLimit/);
  assert.match(src, /updateRateLimit/);
  assert.doesNotMatch(src, /loadCapabilityAdminDashboard/);
  assert.match(src, /data-testid="engine-capabilities-panel"/);
  assert.equal(/\.streamChat\s*\(/.test(src), false);
  assert.equal(/cmd\(\s*['"]stream_chat['"]/.test(src), false);
});

test('EngineCapabilitiesPanel is a run evidence projection, not a scheduler inventory', () => {
  const src = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/EngineCapabilitiesPanel.tsx'),
    'utf8',
  );
  assert.match(src, /selectedRun/);
  assert.match(src, /runSnapshot/);
  assert.match(src, /getCapabilities/);
  assert.match(src, /hasMethod/);
  assert.match(src, /listCapabilitySkills/);
  assert.match(src, /listCapabilityMcpServers/);
  assert.match(src, /classifyError/);
  assert.match(src, /capability_snapshot/);
  assert.match(src, /effective_prompt_hash/);
  assert.match(src, /prompt_plan\.layers/);
  assert.match(src, /tool_plan\.canonical_hash/);
  assert.match(src, /discovered_not_executable/);
  assert.match(src, /href="\/jobs"/);
  assert.match(src, /Loadable/);
  assert.doesNotMatch(src, /listScheduler/);
  assert.doesNotMatch(src, /scheduler\.list/);
  assert.doesNotMatch(src, /SchedulerAdminSnapshot/);
});

test('Engine capability evidence types and bilingual copy cover the run projection', () => {
  const canvasModel = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/nativeExecutionCanvasModel.ts'),
    'utf8',
  );
  const workspace = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );
  const en = fs.readFileSync(path.join(process.cwd(), 'src/i18n/en.ts'), 'utf8');
  const zh = fs.readFileSync(path.join(process.cwd(), 'src/i18n/zh.ts'), 'utf8');

  assert.match(canvasModel, /effective_prompt_hash/);
  assert.match(canvasModel, /PromptLayerEvidence/);
  assert.match(workspace, /permission_profile/);
  assert.match(workspace, /runtime_id/);
  assert.match(workspace, /capability_snapshot/);
  assert.match(en, /engineCapabilities:\s*\{/);
  assert.match(zh, /engineCapabilities:\s*\{/);
});

test('Settings exposes one execution-engine entry containing capabilities and Harness', () => {
  const nav = fs.readFileSync(
    path.join(process.cwd(), 'src/components/shell/settings-navigation.ts'),
    'utf8',
  );
  const page = fs.readFileSync(
    path.join(process.cwd(), 'src/components/shell/SettingsPage.tsx'),
    'utf8',
  );
  const sidebar = fs.readFileSync(
    path.join(process.cwd(), 'src/components/shell/Sidebar.tsx'),
    'utf8',
  );
  const workspace = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );
  assert.match(nav, /section === 'engineering' \|\| section === 'engine'/);
  assert.match(page, /case 'runtime'/);
  assert.match(page, /NativeHarnessPanel/);
  assert.doesNotMatch(page, /case 'engine'/);
  assert.match(sidebar, /id: 'runtime'/);
  assert.doesNotMatch(sidebar, /id: 'engineering'/);
  assert.doesNotMatch(sidebar, /id: 'engine'/);
  assert.match(workspace, /EngineCapabilitiesPanel/);
  assert.match(workspace, /selectedRun=/);
  assert.match(workspace, /runSnapshot=/);
  assert.match(workspace, /tab === 'runs' \|\| tab === 'capabilities'/);
});
