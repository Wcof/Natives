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
  assert.match(src, /loadCapabilityAdminDashboard/);
  assert.match(src, /data-testid="engine-capabilities-panel"/);
  assert.equal(/\.streamChat\s*\(/.test(src), false);
  assert.equal(/cmd\(\s*['"]stream_chat['"]/.test(src), false);
});

test('Settings navigation and page mount engine section', () => {
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
  assert.match(nav, /'engine'/);
  assert.match(page, /EngineCapabilitiesPanel/);
  assert.match(page, /case 'engine'/);
  assert.match(sidebar, /id: 'engine'/);
});
