// ─── Conversation Sidebar ───────────────────────────────
//
// Left panel showing conversation list with search, create, archive.

import { useState } from 'react';
import { MessageSquare, Plus, Search, Archive, Trash2 } from 'lucide-react';

interface Conversation {
  id: string;
  title: string;
  mode: 'chat' | 'agent';
  updatedAt: string;
  messageCount?: number;
}

interface ConversationSidebarProps {
  conversations: Conversation[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onArchive: (id: string) => void;
  onDelete: (id: string) => void;
  locale: string;
}

export default function ConversationSidebar({
  conversations, selectedId, onSelect, onCreate, onArchive, onDelete, locale,
}: ConversationSidebarProps) {
  const [query, setQuery] = useState('');

  const filtered = query.trim()
    ? conversations.filter(c => c.title.toLowerCase().includes(query.toLowerCase()))
    : conversations;

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border)]">
        <h2 className="text-sm font-semibold">
          {locale.startsWith('zh') ? '会话' : 'Conversations'}
        </h2>
        <button
          onClick={onCreate}
          className="p-1.5 rounded-lg hover:bg-[var(--surface-hover)] transition-colors"
          aria-label={locale.startsWith('zh') ? '新建会话' : 'New conversation'}
        >
          <Plus size={16} />
        </button>
      </div>

      {/* Search */}
      <div className="px-3 py-2">
        <div className="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-[var(--surface-hover)] text-sm">
          <Search size={14} className="text-[var(--text-disabled)]" />
          <input
            type="text"
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder={locale.startsWith('zh') ? '搜索会话...' : 'Search...'}
            className="flex-1 bg-transparent outline-none text-sm text-[var(--text-primary)] placeholder:text-[var(--text-disabled)]"
          />
        </div>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto px-2 pb-2 space-y-0.5">
        {filtered.length === 0 ? (
          <div className="px-3 py-8 text-center text-sm text-[var(--text-disabled)]">
            {locale.startsWith('zh') ? '暂无会话' : 'No conversations'}
          </div>
        ) : (
          filtered.map(conv => (
            <button
              key={conv.id}
              onClick={() => onSelect(conv.id)}
              className={`w-full flex items-center gap-2.5 px-3 py-2.5 rounded-lg text-left transition-all text-sm ${
                selectedId === conv.id
                  ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                  : 'hover:bg-[var(--surface-hover)] text-[var(--text-secondary)]'
              }`}
            >
              <MessageSquare size={14} className="shrink-0" />
              <div className="flex-1 min-w-0">
                <div className="truncate font-medium">{conv.title}</div>
                <div className="text-xs text-[var(--text-disabled)] mt-0.5">
                  {conv.mode === 'agent' ? 'Agent' : 'Chat'}
                  {conv.messageCount !== undefined && ` · ${conv.messageCount} msgs`}
                </div>
              </div>
            </button>
          ))
        )}
      </div>
    </div>
  );
}