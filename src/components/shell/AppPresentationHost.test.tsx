import assert from 'node:assert/strict';
import React from 'react';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import AppPresentationHost from './AppPresentationHost';
import type { ActiveAppTarget } from '@/lib/app-target';
import type { AppView } from '@/lib/tauri/apps';

const mockWebView: AppView = {
  appId: 'app-web-1',
  title: 'Test Web App',
  kind: 'web_application',
  registrationOrigin: 'manual',
  showInSidebar: true,
  sidebarOrder: 1,
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

const mockTarget: ActiveAppTarget = {
  appId: 'app-web-1',
  kind: 'web_application',
  phase: 'presented',
  view: mockWebView,
  error: null,
};

describe('AppPresentationHost Unit and Regression Tests', () => {
  it('renders web toolbar with title and navigation controls', () => {
    const html = renderToStaticMarkup(
      React.createElement(AppPresentationHost, {
        target: mockTarget,
        locale: 'zh',
        onRetry: () => {},
        onBackToApps: () => {},
        onWebCommand: () => {},
        onSystemCommand: () => {},
      })
    );

    assert.ok(html.includes('Test Web App'), 'Should render web app title');
    assert.ok(html.includes('data-app-presentation="app-web-1"'), 'Should render container with app id attribute');
  });

  it('renders error state when phase is error', () => {
    const errorTarget: ActiveAppTarget = {
      appId: 'app-web-1',
      kind: 'web_application',
      phase: 'error',
      view: mockWebView,
      error: 'Failed to load page',
    };

    const html = renderToStaticMarkup(
      React.createElement(AppPresentationHost, {
        target: errorTarget,
        locale: 'zh',
        onRetry: () => {},
        onBackToApps: () => {},
        onWebCommand: () => {},
        onSystemCommand: () => {},
      })
    );

    assert.ok(html.includes('Failed to load page'), 'Should display error text');
  });
});
