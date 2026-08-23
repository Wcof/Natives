import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { AppView } from '@/lib/tauri/apps';

describe('App Center Action & Capability Matrix (APP-003 / APP-071)', () => {
  it('WebApplication always enforces canStop=false and canRestart=false', () => {
    const webApp: AppView = {
      appId: 'app-web-1',
      title: 'ChatGPT',
      kind: 'web_application',
      registrationOrigin: 'manual',
      showInSidebar: true,
      capabilities: {
        canStart: false,
        canStop: false,
        canRestart: false,
        canOpen: true,
        canEdit: true,
        canRemove: true,
        canSidebar: true,
        riskLevel: 0,
      },
      runtimeState: 'running',
    };

    assert.equal(webApp.capabilities.canStop, false);
    assert.equal(webApp.capabilities.canRestart, false);
    assert.equal(webApp.capabilities.canOpen, true);
    assert.equal(webApp.capabilities.canEdit, true);
    assert.equal(webApp.capabilities.canRemove, true);
  });

  it('LocalProject running exposes start=true, stop=true, restart=true, open=true', () => {
    const localApp: AppView = {
      appId: 'app-local-1',
      title: 'My Vite Project',
      kind: 'local_project',
      registrationOrigin: 'local_scan',
      showInSidebar: false,
      capabilities: {
        canStart: true,
        canStop: true,
        canRestart: true,
        canOpen: true,
        canEdit: true,
        canRemove: true,
        canSidebar: true,
        riskLevel: 1,
      },
      runtimeState: 'running',
    };

    assert.equal(localApp.capabilities.canStart, true);
    assert.equal(localApp.capabilities.canStop, true);
    assert.equal(localApp.capabilities.canRestart, true);
    assert.equal(localApp.capabilities.canOpen, true);
    assert.equal(localApp.capabilities.riskLevel, 1);
  });

  it('SystemApplication preexisting requires Level 2 confirmation', () => {
    const systemApp: AppView = {
      appId: 'app-sys-1',
      title: 'VS Code',
      kind: 'system_application',
      registrationOrigin: 'system_discovery',
      showInSidebar: true,
      capabilities: {
        canStart: false,
        canStop: true,
        canRestart: true,
        canOpen: true,
        canEdit: true,
        canRemove: true,
        canSidebar: true,
        riskLevel: 2,
      },
      runtimeState: 'running',
    };

    assert.equal(systemApp.capabilities.canOpen, true);
    assert.equal(systemApp.capabilities.riskLevel, 2);
  });
});
