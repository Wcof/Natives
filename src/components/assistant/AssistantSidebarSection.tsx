'use client';

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  Archive,
  Copy,
  Folder,
  FolderOpen,
  FolderSearch,
  Loader2,
  MessageSquare,
  MoreHorizontal,
  Pencil,
  Pin,
  PinOff,
  Plus,
  Trash2,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import Portal from '@/components/ui/Portal';
import { useToast } from '@/components/ui/Toast';
import { createTempSession, isTempConversationId } from '@/lib/assistant-temp-conversation';
import { copyToClipboard } from '@/lib/clipboard';
import {
  useAssistantActions,
  useAssistantNavigation,
} from './AssistantWorkspaceContext';

interface AssistantSidebarSectionProps {
  locale: Locale;
  activeNavigationId?: string | null;
  onNavigateAssistant: () => void;
}

const MENU_WIDTH = 160; // w-40
const MENU_GAP = 4;
const MENU_PAD = 8;
/** Approx panel height for flip (pin/rename/archive/copy-id/delete). */
const MENU_EST_HEIGHT = 196;

function computeMenuPos(trigger: HTMLElement): { top: number; left: number; flip: boolean } {
  const rect = trigger.getBoundingClientRect();
  const left = Math.min(
    Math.max(MENU_PAD, rect.right - MENU_WIDTH),
    window.innerWidth - MENU_WIDTH - MENU_PAD,
  );
  const spaceBelow = window.innerHeight - rect.bottom - MENU_PAD;
  const flip = spaceBelow < MENU_EST_HEIGHT && rect.top > MENU_EST_HEIGHT;
  return {
    top: flip ? rect.top - MENU_GAP : rect.bottom + MENU_GAP,
    left,
    flip,
  };
}

export default function AssistantSidebarSection({ locale, activeNavigationId, onNavigateAssistant }: AssistantSidebarSectionProps) {
  // Split hooks: do NOT subscribe to runtime. Stream ticks (events/usage/status)
  // must not re-render the project/conversation tree while a run is active.
  const { navigation, publishNavigation } = useAssistantNavigation();
  const { actions } = useAssistantActions();
  const { toast } = useToast();
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [pinned, setPinned] = useState<Set<string>>(new Set());
  const [menuId, setMenuId] = useState<string | null>(null);
  const [menuPos, setMenuPos] = useState<{ top: number; left: number; flip: boolean } | null>(null);
  const menuTriggerRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const menuPanelRef = useRef<HTMLDivElement>(null);
  const [renameTarget, setRenameTarget] = useState<{ id: string; title: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; title: string } | null>(null);
  const [deletingConversation, setDeletingConversation] = useState(false);
  const [removeProjectTarget, setRemoveProjectTarget] = useState<{ id: string; label: string; path: string } | null>(null);
  const [removingProject, setRemovingProject] = useState(false);
  const [renameProjectTarget, setRenameProjectTarget] = useState<{ id: string; path: string; label: string } | null>(null);
  const [renamingProject, setRenamingProject] = useState(false);
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);

  // Load persisted collapsed state
  useEffect(() => {
    void window.nativesAPI?.db?.get('assistant:collapsedProjects').then(value => {
      if (!value) return;
      try { setCollapsed(new Set(JSON.parse(String(value)) as string[])); } catch { /* ignore corrupt preference */ }
    });
  }, []);

  useEffect(() => {
    void window.nativesAPI?.db?.get('assistant:pinnedProjects').then(value => {
      if (!value) return;
      try { setPinned(new Set(JSON.parse(String(value)) as string[])); } catch { /* ignore corrupt preference */ }
    });
  }, []);

  const persistCollapsed = useCallback((next: Set<string>) => {
    setCollapsed(next);
    void window.nativesAPI?.db?.set('assistant:collapsedProjects', JSON.stringify([...next]));
  }, []);

  const toggleProject = useCallback((id: string) => {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id); else next.add(id);
    persistCollapsed(next);
  }, [collapsed, persistCollapsed]);

  const togglePinned = useCallback((id: string) => {
    const next = new Set(pinned);
    if (next.has(id)) next.delete(id); else next.add(id);
    setPinned(next);
    void window.nativesAPI?.db?.set('assistant:pinnedProjects', JSON.stringify([...next]));
  }, [pinned]);

  const orderedGroups = useMemo(() => {
    // Temp shells must never appear in the persistent project/conversation tree.
    const groups = navigation.groups.map((group) => ({
      ...group,
      conversations: group.conversations.filter((c) => !isTempConversationId(c.id)),
    }));
    return groups
      .map((group, index) => ({ group, index }))
      .sort((left, right) => Number(pinned.has(right.group.id)) - Number(pinned.has(left.group.id)) || left.index - right.index)
      .map(item => item.group);
  }, [navigation.groups, pinned]);

  // Build flat list of all conversation items for keyboard navigation
  const flatItems = useMemo(() => {
    const items: Array<{ type: 'conversation'; id: string; groupId: string }> = [];
    for (const group of orderedGroups) {
      if (collapsed.has(group.id)) continue;
      for (const conv of group.conversations) {
        items.push({ type: 'conversation', id: conv.id, groupId: group.id });
      }
    }
    return items;
  }, [orderedGroups, collapsed]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      setMenuId(null);
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      setFocusedIndex(prev => {
        if (flatItems.length === 0) return null;
        if (prev === null) return e.key === 'ArrowDown' ? 0 : flatItems.length - 1;
        return e.key === 'ArrowDown'
          ? Math.min(prev + 1, flatItems.length - 1)
          : Math.max(prev - 1, 0);
      });
    }
    if (e.key === 'Enter' && focusedIndex !== null && flatItems[focusedIndex]) {
      const item = flatItems[focusedIndex];
      onNavigateAssistant();
      if (actions) {
        actions.selectConversation(item.id);
      } else {
        publishNavigation({ ...navigation, selectedId: item.id });
      }
    }
  }, [flatItems, focusedIndex, onNavigateAssistant, actions]);

  // Focus the conversation item when focusedIndex changes
  useEffect(() => {
    if (focusedIndex !== null && flatItems[focusedIndex]) {
      const item = flatItems[focusedIndex];
      const btn = document.querySelector<HTMLButtonElement>(`[data-conv-id="${item.id}"]`);
      btn?.focus();
    }
  }, [focusedIndex, flatItems]);

  // Position the open menu against its trigger (viewport coords for fixed portal).
  const repositionMenu = useCallback(() => {
    if (!menuId) {
      setMenuPos(null);
      return;
    }
    const trigger = menuTriggerRefs.current.get(menuId);
    if (!trigger) {
      setMenuPos(null);
      return;
    }
    setMenuPos(computeMenuPos(trigger));
  }, [menuId]);

  useLayoutEffect(() => {
    repositionMenu();
  }, [repositionMenu]);

  useEffect(() => {
    if (!menuId) return;
    const onScrollOrResize = () => repositionMenu();
    window.addEventListener('resize', onScrollOrResize);
    // capture: nested sidebar overflow scroll also moves the trigger
    window.addEventListener('scroll', onScrollOrResize, true);
    return () => {
      window.removeEventListener('resize', onScrollOrResize);
      window.removeEventListener('scroll', onScrollOrResize, true);
    };
  }, [menuId, repositionMenu]);

  // Close menus on outside pointerdown. Trigger + portaled panel both use
  // [data-assistant-menu] so item clicks are not treated as outside.
  useEffect(() => {
    if (!menuId) return;
    const handler = (e: PointerEvent) => {
      const target = e.target as HTMLElement | null;
      if (target?.closest?.('[data-assistant-menu]')) return;
      setMenuId(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuId(null);
    };
    document.addEventListener('pointerdown', handler, true);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', handler, true);
      document.removeEventListener('keydown', onKey);
    };
  }, [menuId]);

  const openMenu = useCallback((id: string, trigger?: HTMLElement | null) => {
    if (trigger) {
      menuTriggerRefs.current.set(id, trigger as HTMLButtonElement);
      setMenuPos(computeMenuPos(trigger));
    }
    setMenuId((value) => (value === id ? null : id));
  }, []);

  // Build item index for `aria-activedescendant`
  const itemIndex = useMemo(() => {
    const map = new Map<string, number>();
    let i = 0;
    for (const group of orderedGroups) {
      if (collapsed.has(group.id)) continue;
      for (const conv of group.conversations) {
        map.set(conv.id, i++);
      }
    }
    return map;
  }, [orderedGroups, collapsed]);

  const confirmDeleteConversation = useCallback(async () => {
    if (!deleteTarget || deletingConversation) return;
    // Only through Workspace actions → Gateway (no direct daemon RPC).
    if (!actions?.deleteConversation) {
      toast(t(locale, 'assistantSidebar.workbenchNotReadyDelete'), 'error');
      return;
    }
    setDeletingConversation(true);
    try {
      const deleted = await actions.deleteConversation(deleteTarget.id);
      if (!deleted) throw new Error(t(locale, 'assistantSidebar.deleteConversationFailed'));
      setDeleteTarget(null);
      toast(t(locale, 'assistantSidebar.conversationDeleted'), 'success');
    } catch (error) {
      toast(error instanceof Error ? error.message : t(locale, 'assistantSidebar.deleteConversationFailed'), 'error');
    } finally {
      setDeletingConversation(false);
    }
  }, [actions, deleteTarget, deletingConversation, toast]);

  return (
    <div className="mb-3">
      {/* ── Project and conversation tree ── */}
      <div className="space-y-0.5" onKeyDown={handleKeyDown} role="listbox" aria-label={t(locale, 'nav.assistant')} aria-activedescendant={focusedIndex !== null ? `conv-${flatItems[focusedIndex]?.id}` : undefined}>
        {/* ── Loading ── */}
        {navigation.loading ? (
          <div className="flex justify-center py-3"><Loader2 size={14} className="animate-spin text-[var(--text-disabled)]" /></div>
        ) : navigation.groups.length === 0 ? (
          <button type="button" onClick={() => {
            // From other modules / empty state: always land on the assistant surface first.
            onNavigateAssistant();
            actions?.addProjectFolder();
          }} className="drag-none w-full rounded-lg px-3 py-3 text-left text-xs text-[var(--text-disabled)] hover:bg-[var(--surface-hover)]">
            {t(locale, 'assistant.chooseProjectToBegin')}
          </button>
        ) : orderedGroups.map(group => {
          const isCollapsed = collapsed.has(group.id);
          const isUnassigned = group.id === 'unassigned' || !group.path;
          return (
            <section key={group.id}
              onContextMenu={(event) => {
                // Right-click project chrome (not conversation rows — those stopPropagation).
                if ((event.target as HTMLElement).closest('[id^="conv-"]')) return;
                if (!group.path) return;
                event.preventDefault();
                const trigger = menuTriggerRefs.current.get(`project:${group.id}`);
                openMenu(`project:${group.id}`, trigger ?? (event.currentTarget as HTMLElement));
              }} className="mt-0.5">
              {/* ── Project Header ── */}
              <div className="group/project relative flex items-center gap-0.5 rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-all">
                <button
                  type="button"
                  onClick={() => toggleProject(group.id)}
                  onDoubleClick={() => {
                    if (group.path) setRenameProjectTarget({ id: group.id, path: group.path, label: group.label });
                  }}
                  aria-expanded={!isCollapsed}
                  title={group.path ?? undefined}
                  className="drag-none flex min-w-0 flex-1 items-center gap-2.5 rounded-lg px-3 py-1.5 text-left text-inherit font-inherit"
                >
                  {isUnassigned ? <MessageSquare size={15} className="shrink-0" /> : isCollapsed ? <Folder size={15} className="shrink-0" /> : <FolderOpen size={15} className="shrink-0" />}
                  <span className="min-w-0 flex-1 truncate text-sm">{group.label}</span>
                  {!isUnassigned && <span className="tabular-nums text-[0.625rem] opacity-75">{group.conversations.length}</span>}
                </button>
                {!isUnassigned && <>
                  <button
                    type="button"
                    onClick={() => {
                      onNavigateAssistant();
                      if (actions) {
                        if (group.path) actions.createConversationInProject(group.path);
                      } else if (group.path) {
                        // Shell-only: local temp shell, no conversation.create.
                        const session = createTempSession({
                          projectId: group.path,
                          title: t(locale, 'assistant.newConversation'),
                        });
                        publishNavigation({
                          ...navigation,
                          activeProjectPath: group.path,
                          selectedId: session.conversation.id,
                          tempSession: session,
                          pendingCreateProjectPath: undefined,
                        });
                      }
                    }}
                    disabled={navigation.isCreatingConversation}
                    aria-label={t(locale, 'assistant.newConversation')}
                    title={t(locale, 'assistant.newConversation')}
                    className="drag-none rounded-md p-1 text-inherit hover:bg-[var(--neutral-0)]/10 dark:hover:bg-[var(--neutral-1000)]/10 disabled:opacity-35 transition-all"
                  >
                    {navigation.isCreatingConversation ? <Loader2 size={12} className="animate-spin" /> : <Plus size={12} />}
                  </button>
                  <div data-assistant-menu className="relative shrink-0">
                    <button
                      ref={(node) => {
                        if (node) menuTriggerRefs.current.set(`project:${group.id}`, node);
                        else menuTriggerRefs.current.delete(`project:${group.id}`);
                      }}
                      type="button"
                      data-assistant-menu
                      aria-label={t(locale, 'common.more')}
                      title={t(locale, 'common.more')}
                      aria-expanded={menuId === `project:${group.id}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        openMenu(`project:${group.id}`, event.currentTarget);
                      }}
                      className="drag-none rounded-md p-1 text-inherit hover:bg-[var(--neutral-0)]/10 dark:hover:bg-[var(--neutral-1000)]/10 transition-all"
                    >
                      <MoreHorizontal size={12} />
                    </button>
                  </div>
                </>}
              </div>

              {/* ── Conversation Items ── */}
              {!isCollapsed && group.conversations.map(conversation => {
                const idx = itemIndex.get(conversation.id);
                const isFocused = focusedIndex === idx;
                const isSelected = activeNavigationId === '__assistant__' && conversation.id === navigation.selectedId;
                return (
                  <div
                    key={conversation.id}
                    id={`conv-${conversation.id}`}
                    role="option"
                    aria-selected={isSelected}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      const trigger =
                        menuTriggerRefs.current.get(`conversation:${conversation.id}`) ??
                        (event.currentTarget as HTMLElement);
                      openMenu(`conversation:${conversation.id}`, trigger);
                    }}
                    className={`group relative ml-5 mt-0.5 flex min-h-8 items-center rounded-lg transition-colors ${
                          isSelected
                            ? 'bg-[var(--surface-active)] text-[var(--text)] font-medium'
                        : isFocused
                          ? 'bg-[var(--surface-hover)] text-[var(--text)]'
                          : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
                    }`}
                  >
                    <button
                      type="button"
                      data-conv-id={conversation.id}
                      onClick={() => {
                        onNavigateAssistant();
                        if (actions) {
                          actions.selectConversation(conversation.id);
                        } else {
                          // Selecting a persisted session clears any local temp shell.
                          publishNavigation({
                            ...navigation,
                            selectedId: conversation.id,
                            tempSession: null,
                          });
                        }
                      }}
                      className="drag-none flex min-w-0 flex-1 items-center gap-2 px-2.5 py-1.5 text-left text-xs leading-4"
                    >
                      <MessageSquare size={11} className="shrink-0" />
                      <span className="truncate">{conversation.title}</span>
                      {conversation.pinned ? <Pin size={10} className="ml-auto shrink-0 opacity-70" aria-hidden /> : null}
                    </button>
                    {/* Trigger only; panel is portaled to body so overflow/z-index parents cannot clip it. */}
                    <div data-assistant-menu className="relative mr-1 shrink-0">
                      <button
                        ref={(node) => {
                          const key = `conversation:${conversation.id}`;
                          if (node) menuTriggerRefs.current.set(key, node);
                          else menuTriggerRefs.current.delete(key);
                        }}
                        type="button"
                        data-assistant-menu
                        aria-label={t(locale, 'common.more')}
                        aria-expanded={menuId === `conversation:${conversation.id}`}
                        onClick={(event) => {
                          event.stopPropagation();
                          openMenu(`conversation:${conversation.id}`, event.currentTarget);
                        }}
                        className="drag-none rounded p-0.5 opacity-0 hover:bg-[var(--surface)] group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity duration-150"
                      >
                        <MoreHorizontal size={11} />
                      </button>
                    </div>
                  </div>
                );
              })}
            </section>
          );
        })}
      </div>

      {/* Portaled action menu — always on top of sidebar / content layers. */}
      {menuId && menuPos && (
        <Portal>
          <div
            ref={menuPanelRef}
            data-assistant-menu
            role="menu"
            className="fixed w-40 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-popup"
            style={{
              top: menuPos.top,
              left: menuPos.left,
              zIndex: 'var(--z-context-menu)',
              transform: menuPos.flip ? 'translateY(-100%)' : undefined,
            }}
          >
            {menuId.startsWith('project:') && (() => {
              const projectId = menuId.slice('project:'.length);
              const group = orderedGroups.find((g) => g.id === projectId);
              if (!group?.path) return null;
              return (
                <>
                  <button type="button" role="menuitem" onClick={() => { togglePinned(group.id); setMenuId(null); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                    {pinned.has(group.id) ? <PinOff size={12} /> : <Pin size={12} />}{pinned.has(group.id) ? t(locale, 'assistant.unpinProject') : t(locale, 'assistant.pinProject')}
                  </button>
                  <button type="button" role="menuitem" onClick={() => { setMenuId(null); setRenameProjectTarget({ id: group.id, path: group.path!, label: group.label }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                    <Pencil size={12} />{t(locale, 'assistant.renameProject')}
                  </button>
                  <button type="button" role="menuitem" onClick={() => { setMenuId(null); void window.nativesAPI?.shell.showItemInFolder(group.path!); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                    <FolderSearch size={12} />{t(locale, 'assistant.showProjectInFinder')}
                  </button>
                  <button type="button" role="menuitem" onClick={() => { setMenuId(null); setRemoveProjectTarget({ id: group.id, label: group.label, path: group.path! }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--danger)] hover:bg-[var(--danger)]/10 transition-all">
                    <Trash2 size={12} />{t(locale, 'assistant.removeProject')}
                  </button>
                </>
              );
            })()}
            {menuId.startsWith('conversation:') && (() => {
              const conversationId = menuId.slice('conversation:'.length);
              let conversation: (typeof orderedGroups)[number]['conversations'][number] | null = null;
              let groupPath: string | null | undefined;
              for (const group of orderedGroups) {
                const found = group.conversations.find((c) => c.id === conversationId);
                if (found) {
                  conversation = found;
                  groupPath = group.path;
                  break;
                }
              }
              if (!conversation) return null;
              return (
                <>
                  <button type="button" role="menuitem" onClick={() => { setMenuId(null); actions?.pinConversation?.(conversation!.id, conversation!.projectId ?? groupPath ?? null, !conversation!.pinned); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                    {conversation.pinned ? <PinOff size={12} /> : <Pin size={12} />}{conversation.pinned ? t(locale, 'assistant.unpinConversation') : t(locale, 'assistant.pinConversation')}
                  </button>
                  <button type="button" role="menuitem" onClick={() => { setMenuId(null); setRenameTarget({ id: conversation!.id, title: conversation!.title }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                    <Pencil size={12} />{t(locale, 'assistant.renameConversation')}
                  </button>
                  <button type="button" role="menuitem" onClick={() => { setMenuId(null); actions?.archiveConversation(conversation!.id); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                    <Archive size={12} />{t(locale, 'assistant.archive')}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      const id = conversation!.id;
                      setMenuId(null);
                      void copyToClipboard(id).then((ok) => {
                        if (ok) toast(t(locale, 'assistant.conversationIdCopied'), 'success');
                        else toast(t(locale, 'assistant.copyConversationIdFailed'), 'error');
                      });
                    }}
                    className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all"
                  >
                    <Copy size={12} />{t(locale, 'assistant.copyConversationId')}
                  </button>
                  <button type="button" role="menuitem" onClick={() => { setMenuId(null); setDeleteTarget({ id: conversation!.id, title: conversation!.title }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--danger)] hover:bg-[var(--danger)]/10 transition-all">
                    <Trash2 size={12} />{t(locale, 'assistant.deleteConversation')}
                  </button>
                </>
              );
            })()}
          </div>
        </Portal>
      )}

      {/* ── Rename Modal ── */}
      <Modal isOpen={Boolean(renameTarget)} onClose={() => setRenameTarget(null)} title={t(locale, 'assistant.renameConversation')} width={400}>
        <form onSubmit={event => { event.preventDefault(); if (!renameTarget?.title.trim()) return; actions?.renameConversation(renameTarget.id, renameTarget.title.trim()); setRenameTarget(null); }}>
          <input autoFocus value={renameTarget?.title ?? ''} onChange={event => setRenameTarget(target => target ? { ...target, title: event.target.value } : null)} className="w-full rounded-lg border border-[var(--border)] bg-[var(--background)] px-3 py-2 text-sm outline-none focus:border-[var(--primary)]" />
          <div className="mt-4 flex justify-end gap-2"><button type="button" onClick={() => setRenameTarget(null)} className="btn btn-sm">{t(locale, 'common.cancel')}</button><button type="submit" className="btn btn-sm btn-primary">{t(locale, 'common.save')}</button></div>
        </form>
      </Modal>

      <Modal isOpen={Boolean(renameProjectTarget)} onClose={() => setRenameProjectTarget(null)} title={t(locale, 'assistant.renameProject')} width={400}>
        <form onSubmit={(event) => void (async () => {
          event.preventDefault();
          const target = renameProjectTarget;
          if (!target || !target.label.trim() || !actions?.renameProject || renamingProject) return;
          setRenamingProject(true);
          const renamed = await actions.renameProject(target.path, target.label.trim());
          setRenamingProject(false);
          if (!renamed) {
            toast(t(locale, 'assistantSidebar.renameProjectFailed'), 'error');
            return;
          }
          setRenameProjectTarget(null);
          toast(t(locale, 'assistantSidebar.projectRenamed'), 'success');
        })()}>
          <input autoFocus value={renameProjectTarget?.label ?? ''} onChange={event => setRenameProjectTarget(target => target ? { ...target, label: event.target.value } : null)} className="w-full rounded-lg border border-[var(--border)] bg-[var(--background)] px-3 py-2 text-sm outline-none focus:border-[var(--primary)]" />
          <div className="mt-4 flex justify-end gap-2"><button type="button" disabled={renamingProject} onClick={() => setRenameProjectTarget(null)} className="btn btn-sm">{t(locale, 'common.cancel')}</button><button type="submit" disabled={renamingProject || !renameProjectTarget?.label.trim()} className="btn btn-sm btn-primary">{renamingProject ? t(locale, 'assistantSidebar.savingShort') : t(locale, 'common.save')}</button></div>
        </form>
      </Modal>

      {/* ── Delete Modal ── */}
      <Modal isOpen={Boolean(deleteTarget)} onClose={() => setDeleteTarget(null)} title={t(locale, 'assistant.deleteConversation')} width={420}>
        <p className="text-sm text-[var(--text-secondary)]">{t(locale, 'assistant.deleteConversationConfirm', { title: deleteTarget?.title || '' })}</p>
        <div className="mt-4 flex justify-end gap-2"><button type="button" disabled={deletingConversation} onClick={() => setDeleteTarget(null)} className="btn btn-sm">{t(locale, 'common.cancel')}</button><button type="button" disabled={deletingConversation} onClick={() => void confirmDeleteConversation()} className="btn btn-sm bg-[var(--danger)] text-[var(--neutral-1000)] disabled:opacity-60">{deletingConversation ? t(locale, 'assistantSidebar.deletingShort') : t(locale, 'common.delete')}</button></div>
      </Modal>

      <Modal isOpen={Boolean(removeProjectTarget)} onClose={() => setRemoveProjectTarget(null)} title={t(locale, 'assistant.removeProject')} width={420}>
        <p className="text-sm text-[var(--text-secondary)]">
          {t(locale, 'assistant.removeProjectConfirm', {
            title: removeProjectTarget?.label || removeProjectTarget?.path || '',
          })}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" onClick={() => setRemoveProjectTarget(null)} className="btn btn-sm">
            {t(locale, 'common.cancel')}
          </button>
          <button
            type="button"
            disabled={removingProject}
            onClick={() => void (async () => {
              if (!removeProjectTarget) return;
              if (!actions?.removeProject) {
                toast(
                  t(locale, 'assistantSidebar.workbenchNotReadyRemove'),
                  'error',
                );
                return;
              }
              setRemovingProject(true);
              const removed = await actions.removeProject(removeProjectTarget.path);
              setRemovingProject(false);
              if (!removed) {
                toast(t(locale, 'assistantSidebar.removeProjectFailed'), 'error');
                return;
              }
              const next = new Set(pinned);
              next.delete(removeProjectTarget.id);
              setPinned(next);
              void window.nativesAPI?.db?.set('assistant:pinnedProjects', JSON.stringify([...next]));
              setRemoveProjectTarget(null);
              toast(t(locale, 'assistantSidebar.projectRemoved'), 'success');
            })()}
            className="btn btn-sm bg-[var(--danger)] text-[var(--neutral-1000)]"
          >
            {removingProject ? t(locale, 'assistantSidebar.removingShort') : t(locale, 'assistantSidebar.removeShort')}
          </button>
        </div>
      </Modal>
    </div>
  );
}
