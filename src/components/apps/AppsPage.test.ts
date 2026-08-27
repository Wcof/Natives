import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { AppView } from '@/lib/tauri/apps';
import { AppDetail } from './AppDetail';

(globalThis as { React?: typeof React }).React = React;

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
      updatedAt: '2026-08-25T00:00:00Z',
    };

    assert.equal(webApp.capabilities.canStop, false);
    assert.equal(webApp.capabilities.canRestart, false);
    assert.equal(webApp.capabilities.canOpen, true);
    assert.equal(webApp.capabilities.canEdit, true);
    assert.equal(webApp.capabilities.canRemove, true);
  });

  it('WebApplication management shows a saved URL model, not runtime sections', () => {
    const webApp: AppView = {
      appId: 'app-web-1',
      title: 'Baidu',
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
      runtimeState: 'stopped',
      updatedAt: '2026-08-25T00:00:00Z',
    };
    const html = renderToStaticMarkup(React.createElement(AppDetail, {
      app: webApp,
      loadingAction: false,
      onOpen: () => {},
      onStart: () => {},
      onStop: () => {},
      onRestart: () => {},
      onEdit: () => {},
      onRemove: () => {},
      onToggleSidebar: () => {},
      onClearData: () => {},
    }));

    assert.ok(html.includes('仅保存网址'));
    assert.ok(!html.includes('<span>运行实例'));
    assert.ok(!html.includes('<span>呈现表面'));
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
      updatedAt: '2026-08-25T00:00:00Z',
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
      updatedAt: '2026-08-25T00:00:00Z',
    };

    assert.equal(systemApp.capabilities.canOpen, true);
    assert.equal(systemApp.capabilities.riskLevel, 2);
  });

  it('WebApplicationSpec and SystemApplicationSpec wire types maintain expected structure', () => {
    const webSpec = {
      applicationId: 'app-web-1',
      url: 'https://chatgpt.com',
      approvedOrigins: ['chatgpt.com', 'oaistatic.com'],
      openBehavior: 'native_webview',
      keepAlive: true,
      createdAt: '2026-08-25T00:00:00Z',
      updatedAt: '2026-08-25T00:00:00Z',
    };

    assert.equal(webSpec.applicationId, 'app-web-1');
    assert.equal(webSpec.url, 'https://chatgpt.com');
    assert.equal(webSpec.approvedOrigins.length, 2);
    assert.equal(webSpec.openBehavior, 'native_webview');
    assert.equal(webSpec.keepAlive, true);

    const systemSpec = {
      applicationId: 'app-sys-1',
      applicationPath: '/Applications/Notes.app',
      bundleIdentifier: 'com.apple.Notes',
      platform: 'macos',
      launchPolicy: 'activate_existing',
      createdAt: '2026-08-25T00:00:00Z',
      updatedAt: '2026-08-25T00:00:00Z',
    };

    assert.equal(systemSpec.applicationId, 'app-sys-1');
    assert.equal(systemSpec.applicationPath, '/Applications/Notes.app');
    assert.equal(systemSpec.bundleIdentifier, 'com.apple.Notes');
    assert.equal(systemSpec.platform, 'macos');
    assert.equal(systemSpec.launchPolicy, 'activate_existing');
  });
});
