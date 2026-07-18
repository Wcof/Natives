import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const sidebar = readFileSync(new URL('./Sidebar.tsx', import.meta.url), 'utf8');
const header = readFileSync(new URL('./Header.tsx', import.meta.url), 'utf8');
const workbench = readFileSync(new URL('../assistant/AssistantWorkbench.tsx', import.meta.url), 'utf8');
const assistantSidebar = readFileSync(new URL('../assistant/AssistantSidebarSection.tsx', import.meta.url), 'utf8');
const messageInput = readFileSync(new URL('../assistant/MessageInput.tsx', import.meta.url), 'utf8');
const layoutEvents = readFileSync(new URL('./hooks/useLayoutEvents.ts', import.meta.url), 'utf8');
const tauriAdapter = readFileSync(new URL('../../lib/tauri-adapter.ts', import.meta.url), 'utf8');

test('assistant tree is owned only by the shell sidebar', () => {
  assert.equal((sidebar.match(/<AssistantSidebarSection/g) ?? []).length, 1);
  assert.equal(workbench.includes('AssistantSidebarSection'), false);
});

test('assistant workbench is a composition layer over Gateway + Store', () => {
  assert.match(workbench, /AssistantStoreProvider/);
  assert.match(workbench, /useAssistantGateway/);
  assert.match(workbench, /data-gateway="1"/);
  // No direct assistant execution from components
  assert.equal(workbench.includes('function v2call'), false);
  assert.equal(/\bstreamChat\s*[:(]/.test(workbench), false);
  assert.equal(/assistantV2\.request\(/.test(workbench), false);
});

test('reselecting the active conversation is a no-op (state preserved in store)', () => {
  assert.match(workbench, /if \(id === stateRef\.current\.activeConversationId\) return;/);
});

test('assistant sidebar does not default to a permanent loading spinner', () => {
  const context = readFileSync(new URL('../assistant/AssistantWorkspaceContext.tsx', import.meta.url), 'utf8');
  assert.match(context, /loading:\s*false/);
  assert.equal(/emptyNavigation[\s\S]*loading:\s*true/.test(context), false);
  assert.match(workbench, /setLoadingConversations\(false\)/);
  assert.match(workbench, /activeRun\?\.id/);
});

test('assistant and Quick Access use the same first-level heading style', () => {
  const headingClass = 'text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]';
  assert.equal((sidebar.match(new RegExp(headingClass.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g')) ?? []).length >= 2, true);
  assert.match(sidebar, /Assistant is a first-level[\s\S]*?<div className="mb-1">[\s\S]*?<div className="flex items-center px-3/);
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

test('assistant projects only toggle their conversations and have no separate search', () => {
  assert.equal(assistantSidebar.includes('searchQuery'), false);
  assert.equal(assistantSidebar.includes('<Search'), false);
  assert.match(assistantSidebar, /onClick=\{\(\) => toggleProject\(group\.id\)\}/);
  assert.match(assistantSidebar, /gap-2\.5 rounded-lg px-3 py-1\.5[\s\S]*?<Folder size=\{15\}/);
  assert.equal(assistantSidebar.includes('actions?.selectProject'), false);
  assert.equal(assistantSidebar.includes('navigation.activeProjectPath'), false);
});

test('assistant project menu exposes pin, reveal, and remove actions', () => {
  assert.match(assistantSidebar, /assistant:pinnedProjects/);
  assert.match(assistantSidebar, /showItemInFolder\(group\.path/);
  assert.match(assistantSidebar, /actions\?\.removeProject\(removeProjectTarget\.path\)/);
  assert.match(assistantSidebar, /PinOff/);
});

test('assistant conversation rows use a neutral selected state and comfortable height', () => {
  assert.match(assistantSidebar, /min-h-8/);
  assert.match(assistantSidebar, /bg-\[var\(--surface-active\)\] text-\[var\(--text\)\]/);
  assert.equal(assistantSidebar.includes('bg-[var(--accent)] text-[var(--accent-ink)]'), false);
  // Unselected session titles use the documented secondary text role (not the undefined tertiary token).
  assert.match(assistantSidebar, /text-\[var\(--text-secondary\)\] hover:bg-\[var\(--surface-hover\)\] hover:text-\[var\(--primary\)\]/);
  assert.equal(assistantSidebar.includes('text-[var(--text-tertiary)]'), false);
});

test('send path uses gateway sendOrQueue (queue while running, no streamChat)', () => {
  assert.match(workbench, /sendOrQueue/);
  assert.match(workbench, /startSubscription/);
  assert.equal(/\bstreamChat\s*[:(]/.test(workbench), false);
  assert.equal(workbench.includes('createAssistantStreamState'), false);
});

test('stop uses gateway cancelRun and does not invent terminal run status', () => {
  assert.match(workbench, /cancelRun/);
  // Terminal status must come from daemon events, not optimistic interrupted
  assert.equal(/setStreamState[\s\S]*status: 'interrupted'/.test(workbench), false);
});

test('assistant composer does not change its border when the textarea focuses', () => {
  assert.equal(messageInput.includes('focus-within:border'), false);
});

test('assistant sidebar deletion goes only through workspace actions (Gateway seam)', () => {
  assert.match(assistantSidebar, /confirmDeleteConversation/);
  assert.match(assistantSidebar, /actions\?\.deleteConversation/);
  assert.match(assistantSidebar, /deletingConversation/);
  assert.equal(/\bassistantV2\b/.test(assistantSidebar.replace(/\/\/[^\n]*/g, '')), false);
  assert.equal(assistantSidebar.includes("request('conversation.delete'"), false);
});

test('slash commands use composer-local positioning and own Enter before sending', () => {
  const slashPopover = readFileSync(new URL('../assistant/SlashCommandPopover.tsx', import.meta.url), 'utf8');
  assert.equal(messageInput.includes('anchorRect'), false);
  assert.match(slashPopover, /bottom-full left-0/);
  assert.match(slashPopover, /addEventListener\('keydown', handleKeyDown, true\)/);
  assert.match(messageInput, /event\.defaultPrevented/);
});

test('locale persistence broadcasts the change consumed by the shell', () => {
  assert.match(tauriAdapter, /setLocale:\s*async[\s\S]*?new CustomEvent\('locale-changed',\s*\{ detail: locale \}\)[\s\S]*?await cmd\('set_locale'/);
  assert.match(layoutEvents, /addEventListener\('locale-changed'/);
});
