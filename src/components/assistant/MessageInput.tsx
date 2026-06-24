'use client';

import { useState, useRef, useEffect, useCallback } from 'react';
import type { Locale } from '@/i18n';
import SlashCommandPopover from './SlashCommandPopover';

interface SlashCommand {
  id: string;
  label: string;
  description: string;
  category: 'system' | 'skill' | 'mcp';
}

interface MessageInputProps {
  locale: Locale;
  onSend: (content: string) => void;
  onStop: () => void;
  isStreaming: boolean;
  disabled?: boolean;
  placeholder?: string;
  noProject?: boolean;
  /** 优先于 disabled：'no_provider' = 无 provider；'creating' = 会话创建中 */
  inputDisabledReason?: 'no_provider' | 'creating' | null;
}

export default function MessageInput({
  locale,
  onSend,
  onStop,
  isStreaming,
  disabled = false,
  placeholder,
  noProject = false,
  inputDisabledReason = null,
}: MessageInputProps) {
  const [input, setInput] = useState('');
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashQuery, setSlashQuery] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const inputContainerRef = useRef<HTMLDivElement>(null);
  const lastSlashIndex = useRef(-1);

  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.style.height = Math.min(textareaRef.current.scrollHeight, 200) + 'px';
    }
  }, [input]);

  // Track slash state from input
  useEffect(() => {
    const slashIdx = input.lastIndexOf('/');
    if (slashIdx >= 0) {
      // Check if there's text before the slash (not at start of line)
      const beforeSlash = input.slice(0, slashIdx);
      const atLineStart = slashIdx === 0 || beforeSlash.endsWith('\n');
      if (atLineStart) {
        setSlashOpen(true);
        setSlashQuery(input.slice(slashIdx + 1));
        lastSlashIndex.current = slashIdx;
      } else {
        setSlashOpen(false);
      }
    } else {
      setSlashOpen(false);
    }
  }, [input]);

  const handleSend = () => {
    const trimmed = input.trim();
    if (!trimmed || isStreaming || effectiveDisabled) return;
    onSend(trimmed);
    setInput('');
    setSlashOpen(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleSlashSelect = useCallback((cmd: SlashCommand) => {
    // Replace the slash query with the full command + space
    const before = input.slice(0, lastSlashIndex.current);
    setInput(`${before}${cmd.id} `);
    setSlashOpen(false);
    textareaRef.current?.focus();
  }, [input]);

  const t = (key: string) => {
    const lang = locale.startsWith('zh') ? 'zh' : 'en';
    const messages: Record<string, Record<string, string>> = {
      send: { zh: '发送', en: 'Send' },
      stop: { zh: '停止生成', en: 'Stop' },
      inputPlaceholder: { zh: '输入消息...', en: 'Type a message...' },
      noProject: { zh: '当前无项目上下文，部分能力受限（如 /create-app 等指令暂不可用）', en: 'No project context, some features limited (e.g. /create-app unavailable)' },
      noProviderHint: { zh: '请先在设置中配置 AI 供应商', en: 'Configure an AI provider first' },
      creatingSession: { zh: '正在创建会话...', en: 'Starting session...' },
    };
    return messages[key]?.[lang] ?? key;
  };

  // inputDisabledReason 优先于 disabled
  const reasonDisabled = inputDisabledReason === 'no_provider' || inputDisabledReason === 'creating';
  const effectiveDisabled = disabled || reasonDisabled;
  const helperText = inputDisabledReason === 'no_provider'
    ? t('noProviderHint')
    : inputDisabledReason === 'creating'
      ? t('creatingSession')
      : null;

  return (
    <div className="flex flex-col gap-2 px-4 py-3 border-t border-[var(--vibe-border-subtle)] relative">
      {noProject && (
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-amber-500/10 border border-amber-500/20 text-xs text-amber-600 dark:text-amber-400">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="8" x2="12" y2="12" />
            <line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
          <span>{t('noProject')}</span>
        </div>
      )}

      <div ref={inputContainerRef} className="relative">
        {/* Slash command popover */}
        <SlashCommandPopover
          isOpen={slashOpen}
          query={slashQuery}
          onSelect={handleSlashSelect}
          onClose={() => setSlashOpen(false)}
          disabled={noProject}
          anchorRect={textareaRef.current?.getBoundingClientRect() ?? null}
        />

        <div className="flex items-end gap-2 rounded-xl border border-[var(--vibe-search-border)] bg-[var(--vibe-search-bg)] px-3 py-2 focus-within:border-[var(--vibe-active-color)] transition-colors">
          <textarea
            ref={textareaRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={helperText || placeholder || t('inputPlaceholder')}
            disabled={effectiveDisabled}
            rows={1}
            className="min-w-0 flex-1 bg-transparent text-sm text-[var(--vibe-brand-text)] outline-none resize-none placeholder:text-[var(--vibe-search-placeholder)] max-h-[200px]"
          />
          {isStreaming ? (
            <button
              onClick={onStop}
              className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-red-500/10 text-red-500 hover:bg-red-500/20 transition-colors text-sm font-medium"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                <rect x="6" y="6" width="12" height="12" rx="2" />
              </svg>
              <span className="hidden sm:inline">{t('stop')}</span>
            </button>
          ) : (
            <button
              onClick={handleSend}
              disabled={!input.trim() || effectiveDisabled}
              className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] hover:opacity-80 transition-opacity text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <line x1="22" y1="2" x2="11" y2="13" />
                <polygon points="22 2 15 22 11 13 2 9 22 2" />
              </svg>
              <span className="hidden sm:inline">{t('send')}</span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
