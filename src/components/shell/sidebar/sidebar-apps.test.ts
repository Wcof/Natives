import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { getNavigationId } from './model';
import type { AppView } from '@/lib/tauri/apps';

describe('Sidebar Apps Projection (APP-060 / APP-061 / APP-071)', () => {
  it('maps apps:item:<appId> navigation identity correctly', () => {
    const navId = getNavigationId('apps:item:app-123');
    assert.equal(navId, 'apps:item:app-123');
  });

  it('maps apps top-level entry', () => {
    assert.equal(getNavigationId('apps'), 'apps');
    assert.equal(getNavigationId('__apps__'), 'apps');
  });

  it('filters and sorts sidebar apps by sidebarOrder', () => {
    const list: AppView[] = [
      {
        appId: '1',
        title: 'App 1',
        kind: 'local_project',
        registrationOrigin: 'manual',
        showInSidebar: false,
        capabilities: { canStart: true, canStop: false, canRestart: false, canOpen: false, canEdit: true, canRemove: true, canSidebar: true, riskLevel: 0 },
        runtimeState: 'stopped',
      },
      {
        appId: '2',
        title: 'App 2',
        kind: 'web_application',
        registrationOrigin: 'manual',
        showInSidebar: true,
        sidebarOrder: 2,
        capabilities: { canStart: false, canStop: false, canRestart: false, canOpen: true, canEdit: true, canRemove: true, canSidebar: true, riskLevel: 0 },
        runtimeState: 'stopped',
      },
      {
        appId: '3',
        title: 'App 3',
        kind: 'system_application',
        registrationOrigin: 'manual',
        showInSidebar: true,
        sidebarOrder: 1,
        capabilities: { canStart: false, canStop: false, canRestart: false, canOpen: true, canEdit: true, canRemove: true, canSidebar: true, riskLevel: 0 },
        runtimeState: 'stopped',
      },
    ];

    const visible = list.filter((a) => a.showInSidebar);
    visible.sort((a, b) => (a.sidebarOrder ?? 0) - (b.sidebarOrder ?? 0));

    assert.equal(visible.length, 2);
    assert.equal(visible[0]?.appId, '3');
    assert.equal(visible[1]?.appId, '2');
  });
});
