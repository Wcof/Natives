/**
 * Regression: stream runtime ticks must not re-render the shell sidebar tree.
 *
 * H1 (2026-07-23): AssistantWorkspaceContext bundled navigation + runtime in one
 * value. publishRuntime on every stream event forced AssistantSidebarSection
 * (and Sidebar) to re-render while a run was active → 会话栏卡顿.
 *
 * Fix: split into Navigation / Runtime / Actions contexts + dedicated hooks.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const contextSrc = readFileSync(
  fileURLToPath(new URL('./AssistantWorkspaceContext.tsx', import.meta.url)),
  'utf8',
);
const sidebarSectionSrc = readFileSync(
  fileURLToPath(new URL('./AssistantSidebarSection.tsx', import.meta.url)),
  'utf8',
);
// R4-04: the shell sidebar's controller logic (including useAssistantActions)
// moved into the useSidebar hook; the shell file only composes presentational parts.
const shellSidebarSrc = readFileSync(
  fileURLToPath(new URL('../shell/sidebar/useSidebar.ts', import.meta.url)),
  'utf8',
);

describe('assistant workspace context split (stream sidebar thrash)', () => {
  it('exposes separate Navigation / Runtime / Actions / Api contexts and hooks', () => {
    assert.match(contextSrc, /AssistantNavigationContext\s*=\s*createContext/);
    assert.match(contextSrc, /AssistantRuntimeContext\s*=\s*createContext/);
    assert.match(contextSrc, /AssistantActionsContext\s*=\s*createContext/);
    assert.match(contextSrc, /AssistantWorkspaceApiContext\s*=\s*createContext/);
    assert.match(contextSrc, /export function useAssistantNavigation/);
    assert.match(contextSrc, /export function useAssistantRuntime/);
    assert.match(contextSrc, /export function useAssistantActions/);
    assert.match(contextSrc, /export function useAssistantWorkspaceApi/);
    // Nested providers so each slice has its own referential value.
    assert.match(contextSrc, /AssistantNavigationContext\.Provider/);
    assert.match(contextSrc, /AssistantRuntimeContext\.Provider/);
    assert.match(contextSrc, /AssistantActionsContext\.Provider/);
    assert.match(contextSrc, /AssistantWorkspaceApiContext\.Provider/);
  });

  it('sidebar conversation tree does not subscribe to combined workspace / runtime', () => {
    assert.match(sidebarSectionSrc, /useAssistantNavigation/);
    assert.match(sidebarSectionSrc, /useAssistantActions/);
    // Must not use the combined hook (it re-renders on runtime ticks).
    assert.equal(sidebarSectionSrc.includes('useAssistantWorkspace'), false);
    assert.equal(sidebarSectionSrc.includes('useAssistantRuntime'), false);
  });

  it('shell Sidebar rail only needs actions, not runtime', () => {
    assert.match(shellSidebarSrc, /useAssistantActions/);
    assert.equal(shellSidebarSrc.includes('useAssistantWorkspace'), false);
    assert.equal(shellSidebarSrc.includes('useAssistantRuntime'), false);
  });

  it('navigation value useMemo does not depend on runtime', () => {
    // navigationValue must only close over navigation + publishNavigation.
    assert.match(
      contextSrc,
      /const navigationValue = useMemo<NavigationContextValue>\(\s*\(\)\s*=>\s*\(\{\s*navigation,\s*publishNavigation\s*\}\),\s*\[navigation,\s*publishNavigation\]/,
    );
    // runtimeValue is state-only (publishers live on the stable Api context).
    assert.match(
      contextSrc,
      /const runtimeValue = useMemo<RuntimeContextValue>\(\s*\(\)\s*=>\s*\(\{\s*runtime\s*\}\),\s*\[runtime\]/,
    );
  });

  it('workbench publishers come from stable Api, not runtime context', () => {
    const workbenchSrc = readFileSync(
      fileURLToPath(new URL('./AssistantWorkbench.tsx', import.meta.url)),
      'utf8',
    );
    assert.match(workbenchSrc, /useAssistantWorkspaceApi/);
    assert.equal(workbenchSrc.includes('useAssistantRuntime'), false);
    assert.equal(workbenchSrc.includes('useAssistantWorkspace('), false);
  });
});
