'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Archive,
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
import { useToast } from '@/components/ui/Toast';
import { useAssistantWorkspace } from './AssistantWorkspaceContext';

interface AssistantSidebarSectionProps {
  locale: Locale;
  activeNavigationId?: string | null;
  onNavigateAssistant: () => void;
}

export default function AssistantSidebarSection({ locale, activeNavigationId, onNavigateAssistant }: AssistantSidebarSectionProps) {
  const { navigation, actions, publishNavigation } = useAssistantWorkspace();
  const { toast } = useToast();
  const zh = locale.startsWith('zh');
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [pinned, setPinned] = useState<Set<string>>(new Set());
  const [menuId, setMenuId] = useState<string | null>(null);
  const [renameTarget, setRenameTarget] = useState<{ id: string; title: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; title: string } | null>(null);
  const [deletingConversation, setDeletingConversation] = useState(false);
  const [removeProjectTarget, setRemoveProjectTarget] = useState<{ id: string; label: string; path: string } | null>(null);
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

  const orderedGroups = useMemo(() => navigation.groups
    .map((group, index) => ({ group, index }))
    .sort((left, right) => Number(pinned.has(right.group.id)) - Number(pinned.has(left.group.id)) || left.index - right.index)
    .map(item => item.group), [navigation.groups, pinned]);

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

  // Close menus on click outside
  useEffect(() => {
    if (!menuId) return;
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest('[data-assistant-menu]')) {
        setMenuId(null);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [menuId]);

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
    setDeletingConversation(true);
    try {
      const deleted = actions
        ? await actions.deleteConversation(deleteTarget.id)
        : await window.nativesAPI?.assistantV2?.request('conversation.delete', { id: deleteTarget.id }).then(() => true) ?? false;
      if (!deleted) throw new Error(zh ? '删除会话失败' : 'Failed to delete conversation');
      if (!actions) {
        publishNavigation({
          ...navigation,
          groups: navigation.groups.map(group => ({
            ...group,
            conversations: group.conversations.filter(conversation => conversation.id !== deleteTarget.id),
          })).filter(group => group.conversations.length > 0 || group.id === 'unassigned'),
          selectedId: navigation.selectedId === deleteTarget.id ? null : navigation.selectedId,
        });
      }
      setDeleteTarget(null);
      toast(zh ? '会话已删除' : 'Conversation deleted', 'success');
    } catch (error) {
      toast(error instanceof Error ? error.message : (zh ? '删除会话失败' : 'Failed to delete conversation'), 'error');
    } finally {
      setDeletingConversation(false);
    }
  }, [actions, deleteTarget, deletingConversation, navigation, publishNavigation, toast, zh]);

  return (
    <div className="mb-3">
      {/* ── Project and conversation tree ── */}
      <div className="space-y-0.5" onKeyDown={handleKeyDown} role="listbox" aria-label={t(locale, 'nav.assistant')} aria-activedescendant={focusedIndex !== null ? `conv-${flatItems[focusedIndex]?.id}` : undefined}>
        {/* ── Loading ── */}
        {navigation.loading ? (
          <div className="flex justify-center py-3"><Loader2 size={14} className="animate-spin text-[var(--text-disabled)]" /></div>
        ) : navigation.groups.length === 0 ? (
          <button type="button" onClick={() => actions?.addProjectFolder()} className="drag-none w-full rounded-lg px-3 py-3 text-left text-xs text-[var(--text-disabled)] hover:bg-[var(--surface-hover)]">
            {t(locale, 'assistant.chooseProjectToBegin')}
          </button>
        ) : orderedGroups.map(group => {
          const isCollapsed = collapsed.has(group.id);
          const isUnassigned = group.id === 'unassigned' || !group.path;
          return (
            <section key={group.id} className="mt-0.5">
              {/* ── Project Header ── */}
              <div className="group/project relative flex items-center gap-0.5 rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-all">
                <button
                  type="button"
                  onClick={() => toggleProject(group.id)}
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
                      } else {
                        publishNavigation({
                          ...navigation,
                          pendingCreateProjectPath: group.path || null,
                        });
                      }
                    }}
                    disabled={navigation.creationState !== 'ready' || navigation.isCreatingConversation}
                    aria-label={t(locale, 'assistant.newConversation')}
                    title={t(locale, 'assistant.newConversation')}
                    className="drag-none rounded-md p-1 text-inherit hover:bg-black/10 dark:hover:bg-white/10 disabled:opacity-35 transition-all"
                  >
                    {navigation.isCreatingConversation ? <Loader2 size={12} className="animate-spin" /> : <Plus size={12} />}
                  </button>
                  <div data-assistant-menu className="relative">
                    <button
                      type="button"
                      aria-label={t(locale, 'common.more')}
                      title={t(locale, 'common.more')}
                      onClick={() => setMenuId(value => value === `project:${group.id}` ? null : `project:${group.id}`)}
                      className="drag-none rounded-md p-1 text-inherit hover:bg-black/10 dark:hover:bg-white/10 transition-all"
                    >
                      <MoreHorizontal size={12} />
                    </button>
                    {menuId === `project:${group.id}` && group.path && (
                      <div className="absolute right-0 top-full z-50 mt-1 w-40 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-popup">
                        <button type="button" onClick={() => { togglePinned(group.id); setMenuId(null); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                          {pinned.has(group.id) ? <PinOff size={12} /> : <Pin size={12} />}{pinned.has(group.id) ? t(locale, 'assistant.unpinProject') : t(locale, 'assistant.pinProject')}
                        </button>
                        <button type="button" onClick={() => { setMenuId(null); void window.nativesAPI?.shell.showItemInFolder(group.path!); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                          <FolderSearch size={12} />{t(locale, 'assistant.showProjectInFinder')}
                        </button>
                        <button type="button" onClick={() => { setMenuId(null); setRemoveProjectTarget({ id: group.id, label: group.label, path: group.path! }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--danger)] hover:bg-[var(--danger)]/10 transition-all">
                          <Trash2 size={12} />{t(locale, 'assistant.removeProject')}
                        </button>
                      </div>
                    )}
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
                    className={`group relative ml-5 mt-0.5 flex min-h-8 items-center rounded-lg transition-colors ${
                          isSelected
                            ? 'bg-[var(--surface-active)] text-[var(--text)] font-medium'
                        : isFocused
                          ? 'bg-[var(--surface-hover)] text-[var(--text)]'
                          : 'text-[var(--text-tertiary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
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
                          publishNavigation({ ...navigation, selectedId: conversation.id });
                        }
                      }}
                      className="drag-none flex min-w-0 flex-1 items-center gap-2 px-2.5 py-1.5 text-left text-xs leading-4"
                    >
                      <MessageSquare size={11} className="shrink-0" />
                      <span className="truncate">{conversation.title}</span>
                    </button>
                    <button
                      type="button"
                      data-assistant-menu
                      aria-label={t(locale, 'common.more')}
                      onClick={() => setMenuId(value => value === `conversation:${conversation.id}` ? null : `conversation:${conversation.id}`)}
                      className="drag-none mr-1 rounded p-0.5 opacity-0 hover:bg-[var(--surface)] group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity duration-150"
                    >
                      <MoreHorizontal size={11} />
                    </button>
                    {menuId === `conversation:${conversation.id}` && (
                      <div className="absolute right-1 top-full z-50 w-36 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-popup">
                        <button type="button" onClick={() => { setMenuId(null); setRenameTarget({ id: conversation.id, title: conversation.title }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                          <Pencil size={12} />{t(locale, 'assistant.renameConversation')}
                        </button>
                        <button type="button" onClick={() => { setMenuId(null); actions?.archiveConversation(conversation.id); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all">
                          <Archive size={12} />{t(locale, 'assistant.archive')}
                        </button>
                        <button type="button" onClick={() => { setMenuId(null); setDeleteTarget({ id: conversation.id, title: conversation.title }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--danger)] hover:bg-[var(--danger)]/10 transition-all">
                          <Trash2 size={12} />{t(locale, 'assistant.deleteConversation')}
                        </button>
                      </div>
                    )}
                  </div>
                );
              })}
            </section>
          );
        })}
      </div>

      {/* ── Rename Modal ── */}
      <Modal isOpen={Boolean(renameTarget)} onClose={() => setRenameTarget(null)} title={t(locale, 'assistant.renameConversation')} width={400}>
        <form onSubmit={event => { event.preventDefault(); if (!renameTarget?.title.trim()) return; actions?.renameConversation(renameTarget.id, renameTarget.title.trim()); setRenameTarget(null); }}>
          <input autoFocus value={renameTarget?.title ?? ''} onChange={event => setRenameTarget(target => target ? { ...target, title: event.target.value } : null)} className="w-full rounded-lg border border-[var(--border)] bg-[var(--background)] px-3 py-2 text-sm outline-none focus:border-[var(--primary)]" />
          <div className="mt-4 flex justify-end gap-2"><button type="button" onClick={() => setRenameTarget(null)} className="btn btn-sm">{t(locale, 'common.cancel')}</button><button type="submit" className="btn btn-sm btn-primary">{t(locale, 'common.save')}</button></div>
        </form>
      </Modal>

      {/* ── Delete Modal ── */}
      <Modal isOpen={Boolean(deleteTarget)} onClose={() => setDeleteTarget(null)} title={t(locale, 'assistant.deleteConversation')} width={420}>
        <p className="text-sm text-[var(--text-secondary)]">{t(locale, 'assistant.deleteConversationConfirm', { title: deleteTarget?.title ?? '' })}</p>
        <div className="mt-4 flex justify-end gap-2"><button type="button" disabled={deletingConversation} onClick={() => setDeleteTarget(null)} className="btn btn-sm">{t(locale, 'common.cancel')}</button><button type="button" disabled={deletingConversation} onClick={() => void confirmDeleteConversation()} className="btn btn-sm bg-[var(--danger)] text-white disabled:opacity-60">{deletingConversation ? (zh ? '删除中…' : 'Deleting…') : t(locale, 'common.delete')}</button></div>
      </Modal>

      <Modal isOpen={Boolean(removeProjectTarget)} onClose={() => setRemoveProjectTarget(null)} title={t(locale, 'assistant.removeProject')} width={420}>
        <p className="text-sm text-[var(--text-secondary)]">{t(locale, 'assistant.removeProjectConfirm', { title: removeProjectTarget?.label ?? '' })}</p>
        <div className="mt-4 flex justify-end gap-2"><button type="button" onClick={() => setRemoveProjectTarget(null)} className="btn btn-sm">{t(locale, 'common.cancel')}</button><button type="button" onClick={() => { if (removeProjectTarget) { actions?.removeProject(removeProjectTarget.path); const next = new Set(pinned); next.delete(removeProjectTarget.id); setPinned(next); void window.nativesAPI?.db?.set('assistant:pinnedProjects', JSON.stringify([...next])); } setRemoveProjectTarget(null); }} className="btn btn-sm bg-[var(--danger)] text-white">{t(locale, 'common.remove')}</button></div>
      </Modal>
    </div>
  );
}
