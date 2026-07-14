'use client';

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Archive,
  Bot,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  Loader2,
  MessageSquare,
  MoreHorizontal,
  Plus,
  Search,
  Trash2,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { useAssistantWorkspace } from './AssistantWorkspaceContext';

interface AssistantSidebarSectionProps {
  locale: Locale;
  onNavigateAssistant: () => void;
}

export default function AssistantSidebarSection({ locale, onNavigateAssistant }: AssistantSidebarSectionProps) {
  const { navigation, actions } = useAssistantWorkspace();
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [menuId, setMenuId] = useState<string | null>(null);
  const [renameTarget, setRenameTarget] = useState<{ id: string; title: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; title: string } | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Load persisted collapsed state
  useEffect(() => {
    void window.nativesAPI?.db?.get('assistant:collapsedProjects').then(value => {
      if (!value) return;
      try { setCollapsed(new Set(JSON.parse(String(value)) as string[])); } catch { /* ignore corrupt preference */ }
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

  const canCreate = navigation.creationState === 'ready' && !navigation.isCreatingConversation && Boolean(actions);
  const creationHint = {
    engine_unavailable: t(locale, 'assistant.engineNotReady'),
    provider_needed: t(locale, 'assistant.noProviderHint'),
    model_needed: t(locale, 'assistant.noModelHint'),
    ready: t(locale, 'assistant.newConversation'),
  }[navigation.creationState];

  // Build flat list of all conversation items for keyboard navigation
  const flatItems = useMemo(() => {
    const items: Array<{ type: 'conversation'; id: string; groupId: string }> = [];
    for (const group of navigation.groups) {
      if (collapsed.has(group.id)) continue;
      for (const conv of group.conversations) {
        const query = searchQuery.toLowerCase();
        if (query && !conv.title.toLowerCase().includes(query)) continue;
        items.push({ type: 'conversation', id: conv.id, groupId: group.id });
      }
    }
    return items;
  }, [navigation.groups, collapsed, searchQuery]);

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
      actions?.selectConversation(item.id);
    }
  }, [flatItems, focusedIndex, onNavigateAssistant, actions]);

  // Focus the searchable item when focusedIndex changes
  useEffect(() => {
    if (focusedIndex !== null && flatItems[focusedIndex]) {
      const item = flatItems[focusedIndex];
      const btn = document.querySelector<HTMLButtonElement>(`[data-conv-id="${item.id}"]`);
      btn?.focus();
    }
  }, [focusedIndex, flatItems]);

  // ── Keyboard shortcuts: ⌘N/Ctrl+N for new conversation ──
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'n') {
        e.preventDefault();
        if (canCreate) {
          onNavigateAssistant();
          actions?.createConversation();
        }
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [canCreate, onNavigateAssistant, actions]);

  // Close menus on click outside
  useEffect(() => {
    if (!menuId) return;
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest('[data-conversation-menu]')) {
        setMenuId(null);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [menuId]);

  // Filter groups by search query
  const filteredGroups = useMemo(() => {
    if (!searchQuery.trim()) return navigation.groups;
    const q = searchQuery.toLowerCase();
    return navigation.groups
      .map(group => ({
        ...group,
        conversations: group.conversations.filter(c => c.title.toLowerCase().includes(q)),
      }))
      .filter(g => g.conversations.length > 0 || g.id === 'unassigned');
  }, [navigation.groups, searchQuery]);

  // Build item index for `aria-activedescendant`
  const itemIndex = useMemo(() => {
    const map = new Map<string, number>();
    let i = 0;
    for (const group of filteredGroups) {
      if (collapsed.has(group.id)) continue;
      for (const conv of group.conversations) {
        map.set(conv.id, i++);
      }
    }
    return map;
  }, [filteredGroups, collapsed]);

  return (
    <div className="mb-3 px-3">
      {/* ── Tree Area (search + projects/conversations) ── */}
      <div className="max-h-72 overflow-y-auto" onKeyDown={handleKeyDown} role="listbox" aria-label={t(locale, 'nav.assistant')} aria-activedescendant={focusedIndex !== null ? `conv-${flatItems[focusedIndex]?.id}` : undefined}>
        {/* ── Create button row ── */}
        <div className="mb-1 flex items-center gap-1 pr-1">
          <button
            type="button"
            disabled={!canCreate}
            onClick={() => { onNavigateAssistant(); actions?.createConversation(); }}
            title={creationHint}
            aria-label={t(locale, 'assistant.newConversation')}
            data-assistant-create
            className="drag-none flex flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-[0.6875rem] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-35 transition-colors duration-150"
          >
            {navigation.isCreatingConversation ? (
              <Loader2 size={11} className="animate-spin" />
            ) : (
              <Plus size={11} />
            )}
            <span>{t(locale, 'assistant.newConversation')}</span>
          </button>
          <button
            type="button"
            onClick={() => { onNavigateAssistant(); actions?.addProjectFolder(); }}
            title={t(locale, 'assistant.chooseProjectDirectory')}
            aria-label={t(locale, 'assistant.chooseProjectDirectory')}
            className="drag-none rounded-md p-1 text-[var(--text-tertiary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)] transition-colors duration-150"
          >
            <FolderOpen size={11} />
          </button>
        </div>
        {/* ── Search Input ── */}
        <div className="relative mb-1.5 pr-1">
          <Search size={11} className="absolute left-2 top-1/2 -translate-y-1/2 text-[var(--text-disabled)] pointer-events-none" />
          <input
            ref={searchInputRef}
            type="text"
            value={searchQuery}
            onChange={e => setSearchQuery(e.target.value)}
            placeholder={t(locale, 'assistant.searchConversations')}
            className="w-full rounded-md border border-[var(--border-subtle)] bg-[var(--surface)] py-1 pl-6 pr-2 text-[0.6875rem] text-[var(--text-secondary)] outline-none placeholder:text-[var(--text-disabled)] focus:border-[var(--primary)] transition-colors"
            aria-label={t(locale, 'assistant.searchConversations')}
          />
        </div>

        {/* ── Loading ── */}
        {navigation.loading ? (
          <div className="flex justify-center py-3"><Loader2 size={14} className="animate-spin text-[var(--text-disabled)]" /></div>
        ) : filteredGroups.length === 0 ? (
          <button type="button" onClick={() => actions?.addProjectFolder()} className="drag-none w-full rounded-lg px-3 py-3 text-left text-xs text-[var(--text-disabled)] hover:bg-[var(--surface-hover)]">
            {t(locale, 'assistant.chooseProjectToBegin')}
          </button>
        ) : filteredGroups.map(group => {
          const isCollapsed = collapsed.has(group.id);
          const isUnassigned = group.id === 'unassigned' || !group.path;
          return (
            <section key={group.id} className="mt-1">
              {/* ── Project Header ── */}
              <button
                type="button"
                onClick={() => {
                  if (isUnassigned) {
                    actions?.selectProject(null);
                  } else {
                    toggleProject(group.id);
                    actions?.selectProject(group.path ?? null);
                  }
                  onNavigateAssistant();
                }}
                title={group.path ?? undefined}
                className={`drag-none flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-[0.6875rem] ${
                  group.path === navigation.activeProjectPath
                    ? 'bg-[var(--accent)] text-[var(--accent-ink)]'
                    : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
                }`}
              >
                {isUnassigned ? (
                  <MessageSquare size={12} className="shrink-0 text-[var(--text-disabled)]" />
                ) : (
                  <>
                    {isCollapsed ? <ChevronRight size={11} className="shrink-0" /> : <ChevronDown size={11} className="shrink-0" />}
                    {isCollapsed ? <Folder size={12} className="shrink-0 text-[var(--text-disabled)]" /> : <FolderOpen size={12} className="shrink-0 text-[var(--text-disabled)]" />}
                  </>
                )}
                <span className="min-w-0 flex-1 truncate font-medium">{group.label}</span>
                {!isUnassigned && (
                  <span className="tabular-nums text-[0.625rem] text-[var(--text-disabled)]">{group.conversations.length}</span>
                )}
              </button>

              {/* ── Conversation Items ── */}
              {!isCollapsed && group.conversations.map(conversation => {
                const idx = itemIndex.get(conversation.id);
                const isFocused = focusedIndex === idx;
                const isSelected = conversation.id === navigation.selectedId;
                return (
                  <div
                    key={conversation.id}
                    id={`conv-${conversation.id}`}
                    role="option"
                    aria-selected={isSelected}
                    className={`group relative ml-4 mt-0.5 flex items-center rounded-md ${
                      isSelected
                        ? 'bg-[var(--accent)] text-[var(--accent-ink)]'
                        : isFocused
                          ? 'bg-[var(--surface-hover)] text-[var(--text)]'
                          : 'text-[var(--text-tertiary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]'
                    }`}
                  >
                    <button
                      type="button"
                      data-conv-id={conversation.id}
                      onClick={() => { onNavigateAssistant(); actions?.selectConversation(conversation.id); }}
                      className="drag-none flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1 text-left text-[0.6875rem] leading-tight"
                    >
                      <MessageSquare size={11} className="shrink-0" />
                      <span className="truncate">{conversation.title}</span>
                    </button>
                    <button
                      type="button"
                      data-conversation-menu
                      aria-label={t(locale, 'common.more')}
                      onClick={() => setMenuId(value => value === conversation.id ? null : conversation.id)}
                      className="drag-none mr-1 rounded p-0.5 opacity-0 hover:bg-[var(--surface)] group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity duration-150"
                    >
                      <MoreHorizontal size={11} />
                    </button>
                    {menuId === conversation.id && (
                      <div className="absolute right-1 top-full z-40 w-36 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-popup">
                        <button type="button" onClick={() => { setMenuId(null); setRenameTarget({ id: conversation.id, title: conversation.title }); }} className="drag-none w-full rounded-md px-2 py-1.5 text-left text-xs hover:bg-[var(--surface-hover)]">{t(locale, 'assistant.renameConversation')}</button>
                        <button type="button" onClick={() => { setMenuId(null); actions?.archiveConversation(conversation.id); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-[var(--surface-hover)]"><Archive size={12} />{t(locale, 'assistant.archive')}</button>
                        <button type="button" onClick={() => { setMenuId(null); setDeleteTarget({ id: conversation.id, title: conversation.title }); }} className="drag-none flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-[var(--danger)] hover:bg-[var(--danger)]/10"><Trash2 size={12} />{t(locale, 'assistant.deleteConversation')}</button>
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
        <div className="mt-4 flex justify-end gap-2"><button type="button" onClick={() => setDeleteTarget(null)} className="btn btn-sm">{t(locale, 'common.cancel')}</button><button type="button" onClick={() => { if (deleteTarget) actions?.deleteConversation(deleteTarget.id); setDeleteTarget(null); }} className="btn btn-sm bg-[var(--danger)] text-white">{t(locale, 'common.delete')}</button></div>
      </Modal>
    </div>
  );
}
