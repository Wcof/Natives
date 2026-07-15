import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const sidebar = readFileSync(new URL('./Sidebar.tsx', import.meta.url), 'utf8');
const header = readFileSync(new URL('./Header.tsx', import.meta.url), 'utf8');
const workbench = readFileSync(new URL('../assistant/AssistantWorkbench.tsx', import.meta.url), 'utf8');

test('assistant tree is owned only by the shell sidebar', () => {
  assert.equal((sidebar.match(/<AssistantSidebarSection/g) ?? []).length, 1);
  assert.equal(workbench.includes('AssistantSidebarSection'), false);
});

test('assistant and Quick Access use the same first-level heading style', () => {
  const headingClass = 'text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]';
  assert.equal((sidebar.match(new RegExp(headingClass.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g')) ?? []).length >= 2, true);
});

test('expanded sidebar owns collapse and header only restores it', () => {
  assert.match(sidebar, /onClick=\{onToggle\}/);
  assert.match(header, /sidebarCollapsed && onToggleSidebar/);
});

test('assistant heading exposes only tree toggle and add project actions', () => {
  const section = sidebar.slice(sidebar.indexOf('Assistant is a first-level'), sidebar.indexOf('Modules section'));
  assert.equal(section.includes('<Bot'), false);
  assert.equal(section.includes('<FolderPlus'), true);
  assert.equal(section.includes('setAssistantExpanded'), true);
});
