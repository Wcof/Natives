import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const sidebar = readFileSync(new URL('./Sidebar.tsx', import.meta.url), 'utf8');
const header = readFileSync(new URL('./Header.tsx', import.meta.url), 'utf8');
const workbench = readFileSync(new URL('../assistant/AssistantWorkbench.tsx', import.meta.url), 'utf8');
const assistantSidebar = readFileSync(new URL('../assistant/AssistantSidebarSection.tsx', import.meta.url), 'utf8');
const messageInput = readFileSync(new URL('../ui/conversation/MessageInput.tsx', import.meta.url), 'utf8');
const slashPopover = readFileSync(new URL('../ui/conversation/SlashCommandPopover.tsx', import.meta.url), 'utf8');
const layoutEvents = readFileSync(new URL('./hooks/useLayoutEvents.ts', import.meta.url), 'utf8');
const tauriAdapter = readFileSync(new URL('../../lib/tauri/host.ts', import.meta.url), 'utf8');
// Send path moved into the composer hook; run control into the lifecycle hook.
const composerHook = readFileSync(
  new URL('../../hooks/useAssistantWorkbenchComposer.ts', import.meta.url),
  'utf8',
);
const lifecycleHook = readFileSync(
  new URL('../../hooks/useAssistantRunLifecycle.ts', import.meta.url),
  'utf8',
);
const runSubscriptionHook = readFileSync(
  new URL('../../hooks/useAssistantRunSubscription.ts', import.meta.url),
  'utf8',
);

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

test('reselecting the active conversation refreshes snapshot without re-setActive', () => {
  // Agent E: re-open still loads snapshot so artifacts recover after completed runs;
  // child selection is cleared, store activeConversationId stays the same root id.
  assert.match(
    workbench,
    /if \(id === stateRef\.current\.activeConversationId\) \{[\s\S]*?openConversation\([\s\S]*?return;/,
  );
  assert.match(workbench, /setSelectedChildConversationId\(null\)/);
});

test('assistant sidebar does not default to a permanent loading spinner', () => {
  const context = readFileSync(new URL('../assistant/AssistantWorkspaceContext.tsx', import.meta.url), 'utf8');
  // Initial snapshot must start with loading:false (honest empty, not a permanent spinner).
  assert.match(context, /const emptyNavigation[\s\S]{0,200}?loading:\s*false/);
  // Boot may flip loading true while fetching — that is fine; permanent default is not.
  assert.match(workbench, /setLoadingConversations\(false\)/);
  assert.match(workbench, /activeRun\?\.id/);
});

test('assistant and Quick Access use the same first-level heading style', () => {
  const headingClass = 'text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]';
  assert.equal((sidebar.match(new RegExp(headingClass.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g')) ?? []).length >= 2, true);
  assert.match(sidebar, /Assistant is a first-level[\s\S]*?<div className="mb-1">[\s\S]*?<div className="flex items-center px-3/);
});

test('sidebar owns Favorites as a first-level rail above Assistant', () => {
  assert.match(sidebar, /useFavorites/);
  assert.match(sidebar, /from '@\/lib\/favorites-client'/);
  assert.match(sidebar, /data-sidebar-favorites/);
  assert.match(sidebar, /sidebar\.favorites/);
  assert.match(sidebar, /favoritesNavTarget/);
  assert.match(sidebar, /removeAndPersistFavorite/);
  // Always visible as a first-level section; empty shows placeholder
  assert.match(sidebar, /sidebar\.noFavorites/);
  assert.match(sidebar, /favorites\.length === 0/);
  // Position: Quick Access → Favorites → Assistant
  const qa = sidebar.indexOf('Quick Access List');
  const fav = sidebar.indexOf('data-sidebar-favorites');
  const asst = sidebar.indexOf('Assistant is a first-level');
  assert.ok(qa >= 0 && fav > qa && asst > fav);
  // Distinct from fixed Quick Access list
  assert.match(sidebar, /QUICK_ACCESS_ITEMS/);
});

test('expanded sidebar owns collapse and header only restores it', () => {
  // Collapse is owned by the sidebar titlebar button (may wrap onToggle for
  // stopPropagation against drag-region); header only restores when collapsed.
  assert.match(sidebar, /onToggle\(\)/);
  assert.match(sidebar, /titlebar-collapse-btn/);
  assert.match(header, /sidebarCollapsed && onToggleSidebar/);
});

test('collapsed sidebar hides body and places expand control in titlebar', () => {
  assert.match(sidebar, /SIDEBAR_COLLAPSED_WIDTH\s*=\s*0/);
  assert.match(sidebar, /sidebar\.expand/);
  assert.match(sidebar, /PanelLeft size=\{15\}/);
});

test('sidebar uses native traffic lights on macOS and fallback controls elsewhere', () => {
  assert.match(sidebar, /usesNativeTrafficLights/);
  assert.match(sidebar, /native-traffic-spacer/);
  assert.match(sidebar, /titlebar-row/);
  assert.match(sidebar, /data-native-traffic/);
  assert.match(sidebar, /window-controls/);
  assert.match(sidebar, /handleWindowAction\('close'\)/);
  assert.match(sidebar, /handleWindowAction\('minimize'\)/);
  assert.match(sidebar, /handleWindowAction\('maximize'\)/);
  // Custom painted traffic lights / zoom long-press menu are gone
  assert.equal(sidebar.includes('mac-traffic-lights'), false);
  assert.equal(sidebar.includes('handleZoomClick'), false);
  // Hydration-safe: platform detection must not run during render
  assert.match(
    sidebar,
    /useState\(false\)[\s\S]*?setUsesNativeTrafficLights\(detectNativeTrafficLights\(\)\)/,
  );
  assert.match(sidebar, /useEffect\(\(\)\s*=>\s*\{[\s\S]*?detectNativeTrafficLights/);
  // Must not call detectNativeTrafficLights at module/render top-level assignment
  assert.equal(
    /const usesNativeTrafficLights\s*=\s*detectNativeTrafficLights\(\)/.test(sidebar),
    false,
  );
});

test('macOS window chrome keeps system traffic lights (decorations + Overlay)', () => {
  const lib = readFileSync(new URL('../../../src-tauri/src/lib.rs', import.meta.url), 'utf8');
  const widget = readFileSync(new URL('../../../src-tauri/src/commands/widget.rs', import.meta.url), 'utf8');
  const macosConf = readFileSync(new URL('../../../src-tauri/tauri.macos.conf.json', import.meta.url), 'utf8');
  const baseConf = readFileSync(new URL('../../../src-tauri/tauri.conf.json', import.meta.url), 'utf8');

  // Platform override must enable decorations + Overlay title bar
  assert.match(macosConf, /"decorations"\s*:\s*true/);
  assert.match(macosConf, /"titleBarStyle"\s*:\s*"Overlay"/);
  assert.match(macosConf, /"hiddenTitle"\s*:\s*true/);
  assert.match(macosConf, /"trafficLightPosition"/);
  // Aligned with titlebar collapse control (see globals.css geometry contract)
  assert.match(macosConf, /"y"\s*:\s*30/);
  assert.match(macosConf, /"x"\s*:\s*20/);

  // Collapse button must NOT live inside a drag region (macOS swallows the click)
  assert.match(sidebar, /titlebar-collapse-btn/);
  assert.match(sidebar, /titlebar-drag-fill/);
  // Whole titlebar-row is no longer a drag region
  assert.equal(
    /className="titlebar-row"[\s\S]{0,200}data-tauri-drag-region/.test(sidebar),
    false,
  );

  // Base conf stays frameless for Win/Linux custom chrome
  assert.match(baseConf, /"decorations"\s*:\s*false/);

  // window-state must NOT restore decorations/visible (poisoned frameless state)
  assert.match(lib, /with_state_flags/);
  assert.match(lib, /StateFlags::SIZE/);
  assert.match(lib, /StateFlags::POSITION/);
  assert.match(lib, /StateFlags::MAXIMIZED/);
  assert.match(lib, /StateFlags::FULLSCREEN/);
  assert.equal(lib.includes('StateFlags::DECORATIONS'), false);
  assert.equal(lib.includes('StateFlags::VISIBLE'), false);

  // Runtime re-assert on setup + first paint
  assert.match(lib, /apply_macos_traffic_lights/);
  assert.match(lib, /set_decorations\(true\)/);
  assert.match(lib, /TitleBarStyle::Overlay/);
  assert.match(widget, /set_decorations\(true\)/);
  assert.match(widget, /TitleBarStyle::Overlay/);
});

test('sidebar right-edge drag resizes width with clamp + double-click reset', () => {
  assert.match(sidebar, /export function clampSidebarWidth/);
  assert.match(sidebar, /SIDEBAR_MIN_WIDTH\s*=\s*200/);
  assert.match(sidebar, /SIDEBAR_MAX_WIDTH\s*=\s*420/);
  assert.match(sidebar, /sidebar-drag-handle/);
  assert.match(sidebar, /handleSidebarDragStart/);
  assert.match(sidebar, /handleSidebarDragDoubleClick/);
  assert.match(sidebar, /SIDEBAR_DEFAULT_WIDTH/);
  assert.match(sidebar, /ev\.clientX - startX/);
});

test('layout events debounce width persistence and flush on unload', () => {
  assert.match(layoutEvents, /LAYOUT_PERSIST_DEBOUNCE_MS/);
  assert.match(layoutEvents, /layoutPersist/);
  assert.match(layoutEvents, /beforeunload/);
  assert.match(layoutEvents, /_state:sidebar/);
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
  assert.match(assistantSidebar, /actions\.removeProject\(removeProjectTarget\.path\)/);
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
  assert.match(composerHook, /sendOrQueue/);
  assert.match(runSubscriptionHook, /startSubscription/);
  assert.equal(/\bstreamChat\s*[:(]/.test(workbench), false);
  assert.equal(workbench.includes('createAssistantStreamState'), false);
});

test('stop uses gateway cancelRun and does not invent terminal run status', () => {
  assert.match(lifecycleHook, /cancelRun/);
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

test('conversation menu panel lives under data-assistant-menu (outside-click must not kill item clicks)', () => {
  // The outside-close handler uses closest('[data-assistant-menu]').
  // Trigger and portaled panel both mark themselves so item clicks are not outside.
  assert.match(assistantSidebar, /data-assistant-menu/);
  assert.match(assistantSidebar, /setDeleteTarget/);
  // Outside-close should use pointerdown (capture) so it pairs cleanly with the wrapper.
  assert.match(assistantSidebar, /addEventListener\('pointerdown'/);
});

test('assistant action menus portal to body above clipping layers', () => {
  // Must use Portal / body so overflow:hidden sidebar cannot clip the menu.
  assert.match(assistantSidebar, /from '@\/components\/ui\/Portal'|from "@\/components\/ui\/Portal"/);
  assert.match(assistantSidebar, /<Portal>/);
  // Fixed positioning + design-token z-index (above right panel / terminal).
  assert.match(assistantSidebar, /position:\s*['"]fixed['"]|className="fixed/);
  assert.match(assistantSidebar, /--z-context-menu/);
  // Inline absolute menus for conversation/project actions should be gone.
  assert.equal(assistantSidebar.includes('absolute right-1 top-full z-50'), false);
  assert.equal(assistantSidebar.includes('absolute right-0 top-full z-50'), false);
});

test('slash commands use composer-local positioning; keyboard owned by MessageInput', () => {
  assert.equal(messageInput.includes('anchorRect'), false);
  assert.match(slashPopover, /bottom-full left-0/);
  // Popover must NOT own document keydown — MessageInput textarea does
  assert.equal(slashPopover.includes("addEventListener('keydown'"), false);
  assert.match(messageInput, /handleTextareaKeyDown|onKeyDown=\{handleTextareaKeyDown\}/);
  assert.match(messageInput, /event\.defaultPrevented/);
});

test('locale persistence broadcasts the change consumed by the shell', () => {
  assert.match(tauriAdapter, /setLocale:[\s\S]*?new CustomEvent\('locale-changed',\s*\{ detail: locale \}\)[\s\S]*?await cmd\('set_locale'/);
  assert.match(layoutEvents, /addEventListener\('locale-changed'/);
});
