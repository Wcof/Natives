// ─── Conversation Timeline ──────────────────────────────
//
// Main center panel showing the conversation message timeline.

import { renderBlocks, type ContentBlock } from './blocks';

interface Message {
  id: string;
  role: 'system' | 'user' | 'assistant';
  contentBlocks: ContentBlock[];
  status: string;
  createdAt: string;
}

interface ConversationTimelineProps {
  messages: Message[];
  loading: boolean;
  locale: string;
}

export default function ConversationTimeline({ messages, loading, locale }: ConversationTimelineProps) {
  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-sm text-[var(--text-disabled)]">
          {locale.startsWith('zh') ? '加载中...' : 'Loading...'}
        </div>
      </div>
    );
  }

  if (messages.length === 0) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <div className="text-sm text-[var(--text-disabled)] mb-2">
            {locale.startsWith('zh') ? '开始新的对话' : 'Start a new conversation'}
          </div>
          <div className="text-xs text-[var(--text-disabled)]">
            {locale.startsWith('zh') ? '发送消息以开始' : 'Send a message to begin'}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      {messages.map((msg) => (
        <div
          key={msg.id}
          className={`flex gap-3 ${msg.role === 'user' ? 'flex-row-reverse' : ''}`}
        >
          {/* Avatar */}
          <div className={`w-7 h-7 rounded-full flex items-center justify-center text-xs font-medium shrink-0 ${
            msg.role === 'user'
              ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
              : 'bg-[var(--surface-hover)] text-[var(--text-secondary)]'
          }`}>
            {msg.role === 'user' ? 'U' : 'A'}
          </div>

          {/* Content */}
          <div className={`flex-1 min-w-0 max-w-[80%] ${msg.role === 'user' ? 'text-right' : ''}`}>
            <div className={`inline-block text-left rounded-2xl px-4 py-2.5 ${
              msg.role === 'user'
                ? 'bg-[var(--primary)] text-white'
                : 'bg-[var(--surface)] border border-[var(--border)]'
            }`}>
              {renderBlocks(msg.contentBlocks)}
            </div>
            {msg.status === 'streaming' && (
              <div className="mt-1 text-xs text-[var(--text-disabled)]">
                {locale.startsWith('zh') ? '生成中...' : 'Streaming...'}
              </div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}