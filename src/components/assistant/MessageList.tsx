'use client';

import type { Locale } from '@/i18n';
import ToolCallBubble from './ToolCallBubble';
import DiffViewer from './DiffViewer';
import { useState, useEffect } from 'react';

interface Message {
  id: string;
  role: string;
  content: string;
  tool_calls?: string | null;
  tool_result?: string | null;
  status: string;
  created_at: string;
  token_count?: number;
}

interface MessageListProps {
  messages: Message[];
  locale: Locale;
  streamingContent?: string;
  streamingToolCall?: string;
  streamingReasoning?: string;
  isStreaming: boolean;
  onRetryBuild?: (msgId: string) => void;
  buildRetryCount?: number;
}

// ── Thinking Block (Reasoning) ──

function ThinkingBlock({ text, done: isDone = false }: { text: string; done?: boolean }) {
  const [expanded, setExpanded] = useState(true);
  return (
    <div className="mb-3 rounded-lg border border-amber-500/20 bg-amber-500/5 overflow-hidden">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 w-full px-3 py-2 text-xs text-amber-400/80 hover:text-amber-300 transition-colors"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M12 2a7 7 0 0 1 7 7c0 2.38-1.19 4.47-3 5.74V17a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1v-2.26C6.19 13.47 5 11.38 5 9a7 7 0 0 1 7-7z" />
          <line x1="9" y1="21" x2="15" y2="21" />
        </svg>
        <span>Thinking Process</span>
        {isDone && <span className="text-green-400 text-[0.625rem]">✅</span>}
        <span className="ml-auto">{expanded ? 'Collapse' : 'Expand'}</span>
      </button>
      {expanded && (
        <div className="px-3 py-2 text-xs text-amber-400/60 leading-relaxed whitespace-pre-wrap border-t border-amber-500/10">
          {text}
        </div>
      )}
    </div>
  );
}

// ── Enhanced Markdown Renderer ──

function parseToolCallsFromContent(content: string): Array<{ name: string; input: Record<string, unknown> }> {
  try {
    const parsed = JSON.parse(content);
    if (Array.isArray(parsed)) return parsed as Array<{ name: string; input: Record<string, unknown> }>;
    return [{ name: 'tool_call', input: parsed }];
  } catch {
    return [];
  }
}

function CodeBlock({ lang, code }: { lang: string; code: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* ignore */ }
  };
  return (
    <div className="my-3 rounded-lg bg-[var(--vibe-btn-bg)] overflow-hidden">
      <div className="flex items-center justify-between gap-2 px-3 py-1.5 bg-black/20">
        <span className="text-[0.625rem] font-mono uppercase tracking-wider text-[var(--text-faint)]">
          {lang || 'code'}
        </span>
        <button
          onClick={handleCopy}
          className="text-[0.625rem] text-[var(--text-faint)] hover:text-[var(--text-dim)] transition-colors px-1.5 py-0.5"
        >
          {copied ? 'Copied' : 'Copy Code'}
        </button>
      </div>
      <pre className="overflow-x-auto p-3 text-sm font-mono leading-relaxed">
        <code>{code}</code>
      </pre>
    </div>
  );
}

function MarkdownRenderer({ content }: { content: string }) {
  const [html, setHtml] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const { marked } = await import('marked');
        // Sanitize: strip script tags
        const raw = await marked.parse(content);
        const sanitized = typeof raw === 'string'
          ? raw.replace(/<script[\s\S]*?<\/script>/gi, '')
          : String(raw);
        setHtml(sanitized);
      } catch {
        // Fallback: render as plain text
        setHtml(`<pre class="whitespace-pre-wrap text-sm">${content.replace(/</g, '&lt;')}</pre>`);
      }
      setLoading(false);
    })();
  }, [content]);

  if (loading) return <div className="text-sm text-[var(--text-dim)]">...</div>;

  // Split into code blocks (preserve custom CodeBlock rendering) and rendered HTML
  // Note: marked output handles code blocks as <pre><code> already
  return (
    <div
      className="prose prose-sm dark:prose-invert max-w-none break-words"
      style={{ whiteSpace: 'normal' }}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

// ── Extract <think> tag from saved content ──

function extractStoredThinkTag(content: string): { clean: string; thinking: string | null } {
  const match = content.match(/^<think>([\s\S]*?)<\/think>\s*/);
  if (match) {
    return { clean: content.slice(match[0].length), thinking: match[1]!.trim() };
  }
  return { clean: content, thinking: null };
}

// ── Build Error Bubble (Self-Heal / Circuit Breaker) ──

function BuildErrorBubble({ count, maxRetries = 3, onRetry, onManualEdit, onAbandon }:
  { count: number; maxRetries?: number; onRetry?: () => void; onManualEdit?: () => void; onAbandon?: () => void }) {
  const isCircuitBroken = count >= maxRetries;
  return (
    <div className="mt-2 rounded-lg border border-red-500/20 bg-red-500/5 p-3 text-sm">
      <div className="flex items-center gap-2">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-red-400">
          <circle cx="12" cy="12" r="10" />
          <line x1="15" y1="9" x2="9" y2="15" />
          <line x1="9" y1="9" x2="15" y2="15" />
        </svg>
        <span className="font-medium text-red-400">Build Failed ({count}/{maxRetries})</span>
      </div>
      <div className="mt-2 text-xs text-[var(--text-dim)]">
        {isCircuitBroken
          ? 'Max retries reached. Please choose an action below.'
          : 'Linter validation failed. Retrying with error context...'}
      </div>
      <div className="mt-2 flex gap-2">
        {!isCircuitBroken && onRetry && (
          <button onClick={onRetry} className="text-[0.625rem] text-red-400 hover:text-red-300 underline transition-colors">
            Retry
          </button>
        )}
        {isCircuitBroken && (
          <>
            <button onClick={onManualEdit} className="text-[0.625rem] text-amber-400 hover:text-amber-300 underline transition-colors">
              Manual Edit
            </button>
            <button onClick={onAbandon} className="text-[0.625rem] text-[var(--text-faint)] hover:text-[var(--text-dim)] underline transition-colors">
              Abandon
            </button>
          </>
        )}
      </div>
    </div>
  );
}

// ── Main MessageList Component ──

export default function MessageList({ messages, locale, streamingContent, streamingToolCall, streamingReasoning, isStreaming, onRetryBuild, buildRetryCount }: MessageListProps) {
  const t = (key: string) => {
    const lang = locale.startsWith('zh') ? 'zh' : 'en';
    const labels: Record<string, Record<string, string>> = {
      assistant: { zh: '助理', en: 'Assistant' },
      user: { zh: '你', en: 'You' },
      system: { zh: '系统', en: 'System' },
      streaming: { zh: '正在生成...', en: 'Generating...' },
      empty: { zh: '你好！我是你的 AI 助理，有什么可以帮助你的？', en: 'Hello! I\'m your AI assistant. How can I help you?' },
    };
    return labels[key]?.[lang] ?? key;
  };

  const roleIcon = (role: string) => {
    switch (role) {
      case 'user':
        return (
          <div className="w-7 h-7 rounded-full bg-[var(--vibe-active-bg)] flex items-center justify-center text-xs font-bold text-[var(--vibe-active-color)]">
            U
          </div>
        );
      case 'assistant':
        return (
          <div className="w-7 h-7 rounded-full bg-purple-500/20 flex items-center justify-center text-xs font-bold text-purple-400">
            AI
          </div>
        );
      case 'system':
        return (
          <div className="w-7 h-7 rounded-full bg-amber-500/20 flex items-center justify-center text-xs font-bold text-amber-400">
            S
          </div>
        );
      default:
        return null;
    }
  };

  if (messages.length === 0 && !isStreaming) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center px-8 py-12">
          <div className="w-16 h-16 mx-auto mb-4 rounded-2xl bg-purple-500/10 flex items-center justify-center">
            <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="text-purple-400">
              <path d="M12 2L2 7l10 5 10-5-10-5z" />
              <path d="M2 17l10 5 10-5" />
              <path d="M2 12l10 5 10-5" />
            </svg>
          </div>
          <p className="text-sm text-[var(--text-dim)]">{t('empty')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4">
      {messages.map((msg) => {
        // For saved messages, extract stored <think> tags
        const { clean: displayContent, thinking: storedThinking } = extractStoredThinkTag(msg.content);
        const parsedToolCalls = msg.tool_calls ? parseToolCallsFromContent(msg.tool_calls) : [];

        return (
          <div key={msg.id} className="flex gap-3 group">
            <div className="shrink-0 mt-1">{roleIcon(msg.role)}</div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-1">
                <span className="text-xs font-medium text-[var(--text-dim)]">{t(msg.role)}</span>
                {msg.status === 'error' && (
                  <span className="text-xs text-red-400 bg-red-500/10 px-1.5 py-0.5 rounded">Error</span>
                )}
                {msg.status === 'interrupted' && (
                  <span className="text-xs text-amber-400 bg-amber-500/10 px-1.5 py-0.5 rounded">Interrupted</span>
                )}
              </div>

              {/* Stored thinking block */}
              {storedThinking && <ThinkingBlock text={storedThinking} />}

              {/* Content */}
              <div className="text-sm text-[var(--vibe-brand-text)]">
                <MarkdownRenderer content={displayContent} />
              </div>

              {/* Token count for assistant messages */}
              {msg.role === 'assistant' && msg.token_count != null && msg.token_count > 0 && (
                <div className="mt-1 text-[0.625rem] text-[var(--text-faint)]">
                  {msg.token_count} tokens used
                </div>
              )}

              {/* Build error with self-heal */}
              {msg.status === 'build_error' && onRetryBuild && (
                <BuildErrorBubble
                  count={buildRetryCount ?? 0}
                  onRetry={() => onRetryBuild(msg.id)}
                />
              )}

              {/* Tool calls as bubbles */}
              {parsedToolCalls.length > 0 && (
                <div className="mt-2 space-y-2">
                  {parsedToolCalls.map((tc, i) => {
                    // 写盘类工具：在 result 气泡下展示 DiffViewer + 回滚按钮（US-6）
                    const isWriteTool = ['write_generated_module', 'create-app', 'modify-app'].includes(tc.name);
                    let diffData: { old: string; new: string; file: string } | null = null;
                    if (isWriteTool && msg.tool_result) {
                      try {
                        const parsed = JSON.parse(msg.tool_result);
                        if (parsed && parsed.oldContent != null && parsed.newContent != null) {
                          diffData = {
                            old: parsed.oldContent,
                            new: parsed.newContent,
                            file: parsed.fileName || tc.input?.name || 'module',
                          };
                        }
                      } catch { /* tool_result 非 JSON，忽略 */ }
                    }
                    return (
                      <div key={i} className="space-y-2">
                        <ToolCallBubble
                          toolName={tc.name}
                          params={JSON.stringify(tc.input, null, 2)}
                          status={msg.tool_result ? 'result' : 'pending'}
                          result={msg.tool_result || undefined}
                          writePath={tc.input?.module_id ? `~/.natives/modules/${tc.input.module_id}` : undefined}
                          contractId={diffData ? undefined : (tc.input?.contract_id as string | undefined)}
                        />
                        {diffData && (
                          <DiffViewer
                            oldContent={diffData.old}
                            newContent={diffData.new}
                            fileName={diffData.file}
                            onRollback={() => {
                              // US-6 一键回滚：用快照旧内容原子写回 index.html
                              const moduleId = tc.input?.moduleId || tc.input?.module_id || diffData?.file?.split('/')?.[0];
                              if (!moduleId) {
                                console.error('Rollback failed: missing moduleId');
                                return;
                              }
                              const api = window.nativesAPI;
                              if (!api?.module?.rollback) {
                                console.error('Rollback API not available');
                                return;
                              }
                              api.module.rollback({ moduleId: String(moduleId), oldContent: diffData.old })
                                .then(() => console.log('Rollback succeeded for', moduleId))
                                .catch((e: unknown) => console.error('Rollback failed:', e));
                            }}
                          />
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
        );
      })}

      {/* Streaming message bubble */}
      {isStreaming && (
        <div className="flex gap-3">
          <div className="shrink-0 mt-1">{roleIcon('assistant')}</div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <span className="text-xs font-medium text-[var(--text-dim)]">{t('assistant')}</span>
              <span className="flex gap-0.5">
                <span className="w-1.5 h-1.5 rounded-full bg-purple-400 animate-bounce" style={{ animationDelay: '0ms' }} />
                <span className="w-1.5 h-1.5 rounded-full bg-purple-400 animate-bounce" style={{ animationDelay: '150ms' }} />
                <span className="w-1.5 h-1.5 rounded-full bg-purple-400 animate-bounce" style={{ animationDelay: '300ms' }} />
              </span>
            </div>

            {/* Streaming reasoning */}
            {streamingReasoning && <ThinkingBlock text={streamingReasoning} />}

            {/* Streaming content */}
            {streamingContent && (
              <div className="text-sm text-[var(--vibe-brand-text)]">
                <MarkdownRenderer content={streamingContent} />
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
