'use client';

import { useState, useCallback, useEffect, useRef } from 'react';
import { t, type Locale } from '@/i18n';
import { useAssistantStream } from './hooks/useAssistantStream';
import MessageList from './MessageList';
import MessageInput from './MessageInput';
import SessionList from './SessionList';
import ModelSelectorDropdown from './ModelSelectorDropdown';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';

interface Message {
  id: string;
  session_id: string;
  role: string;
  content: string;
  tool_calls?: string | null;
  tool_result?: string | null;
  status: string;
  token_count: number;
  created_at: string;
  sequence: number;
}

interface Session {
  id: string;
  project_id?: string | null;
  title: string;
  model_id: string;
  provider_id: string;
  created_at: string;
  updated_at: string;
  message_count: number;
}

interface ProviderInfo {
  id: string;
  name: string;
  presetName: string;
  baseUrl: string;
  keys: Array<{ id: string; label: string; maskedKey: string }>;
}

interface AssistantWorkbenchProps {
  locale: Locale;
}

export default function AssistantWorkbench({ locale }: AssistantWorkbenchProps) {
  const { toast } = useToast();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [messages, setMessages] = useState<Message[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [noProject, setNoProject] = useState(false);
  const [activeProjectPath, setActiveProjectPath] = useState<string | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const persistedRef = useRef(false);
  const [selectedModel, setSelectedModel] = useState('gpt-4o');
  const [selectedProviderId, setSelectedProviderId] = useState('');
  // Q3-B-1: 'no_provider' = 无 provider 禁用；'creating' = 会话创建中；null = 可用
  const [inputDisabledReason, setInputDisabledReason] = useState<'no_provider' | 'creating' | null>('creating');

  const { streamState, resetStream } = useAssistantStream({
    sessionId: activeSessionId,
    locale,
  });

  // Q2-A + Q3-B + Q3-B-1 + Q4-A: mount 时自动建草稿会话；无 provider 禁用输入并提示
  useEffect(() => {
    let cancelled = false;
    (async () => {
      // 1. 加载项目路径 + 既有 sessions
      const projPath = localStorage.getItem('natives:active_project_path') || null;
      if (cancelled) return;
      setActiveProjectPath(projPath);
      setNoProject(!projPath);
      loadSessions(projPath);

      // 2. Q3-B: 等 providers 就绪
      const providersList = (await window.nativesAPI?.provider?.list?.() as ProviderInfo[] | undefined) ?? [];
      if (cancelled) return;
      setProviders(providersList);

      // 3. Q3-B-1: 无 provider → 禁用输入 + 提示配置
      if (providersList.length === 0) {
        setInputDisabledReason('no_provider');
        setLoading(false);
        return;
      }

      // 4. 初始化默认模型/provider
      if (!selectedProviderId) {
        setSelectedProviderId(providersList[0]!.id);
        const models = getPresetModels(providersList[0]!.presetName);
        if (models.length > 0) setSelectedModel(models[0]!);
      }

      // 5. Q2-A: 自动建草稿会话（project_id = null = 全局草稿）
      try {
        const session = await window.nativesAPI?.assistant?.createSession({
          projectId: null,
          title: '',
          modelId: selectedModel,
          providerId: providersList[0]!.id,
        }) as Session | undefined;
        if (cancelled || !session) return;
        setActiveSessionId(session.id);
        setSessions((prev) => [session, ...prev]);
        setInputDisabledReason(null);
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
        setInputDisabledReason(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
    // Q4-A: 仅 mount 触发，不依赖 currentProjectId
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Scroll to bottom when messages or stream content changes
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, streamState.content]);

  const getPresetModels = (presetName: string): string[] => {
    const map: Record<string, string[]> = {
      'openai': ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo', 'gpt-3.5-turbo'],
      'anthropic': ['claude-sonnet-4-20250514', 'claude-3-5-sonnet-latest', 'claude-3-opus-latest'],
      'ollama': ['llama3', 'mistral', 'codellama', 'qwen2'],
    };
    return map[presetName] || ['gpt-4o'];
  };

  const loadSessions = async (projPath: string | null) => {
    setLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.assistant) {
        const result = await api.assistant.listSessions({ projectId: projPath });
        setSessions(result as Session[]);
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setLoading(false);
    }
  };

  const loadMessages = useCallback(async (sessionId: string) => {
    try {
      const api = window.nativesAPI;
      if (api?.assistant) {
        const result = await api.assistant.getMessages(sessionId);
        setMessages(result as Message[]);
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, []);

  const handleSessionSelect = useCallback(async (sessionId: string) => {
    setActiveSessionId(sessionId);
    persistedRef.current = false;
    resetStream();
    await loadMessages(sessionId);
    // Restore session model settings
    const session = sessions.find(s => s.id === sessionId);
    if (session) {
      setSelectedModel(session.model_id);
      setSelectedProviderId(session.provider_id);
    }
  }, [loadMessages, resetStream, sessions]);

  const handleNewSession = useCallback(async () => {
    try {
      const api = window.nativesAPI;
      if (api?.assistant) {
        const session = await api.assistant.createSession({
          projectId: activeProjectPath,
          title: '',
          modelId: selectedModel,
          providerId: selectedProviderId,
        });
        setSessions((prev) => [session as Session, ...prev]);
        setActiveSessionId((session as Session).id);
        setMessages([]);
        persistedRef.current = false;
        resetStream();
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [activeProjectPath, selectedModel, selectedProviderId, resetStream]);

  const handleDeleteSession = useCallback(async (sessionId: string) => {
    try {
      const api = window.nativesAPI;
      if (api?.assistant) {
        await api.assistant.deleteSession(sessionId);
        setSessions((prev) => prev.filter((s) => s.id !== sessionId));
        if (activeSessionId === sessionId) {
          setActiveSessionId(null);
          setMessages([]);
        }
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [activeSessionId]);

  const handleModelSelect = useCallback(async (providerId: string, model: string) => {
    setSelectedProviderId(providerId);
    setSelectedModel(model);
    if (activeSessionId) {
      try {
        await window.nativesAPI?.assistant?.updateSessionModel({
          sessionId: activeSessionId,
          modelId: model,
          providerId,
        });
        // Update local session cache
        setSessions((prev) => prev.map(s =>
          s.id === activeSessionId ? { ...s, model_id: model, provider_id: providerId } : s
        ));
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
      }
    }
  }, [activeSessionId]);

  const handleSend = useCallback(async (content: string) => {
    if (!activeSessionId) return;

    // Save user message
    try {
      const api = window.nativesAPI;
      if (api?.assistant) {
        await api.assistant.saveMessage({
          sessionId: activeSessionId,
          role: 'user',
          content,
          status: 'done',
          tokenCount: 0,
        });
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }

    // Reload messages to show the new one
    await loadMessages(activeSessionId);
    persistedRef.current = false;

    const model = selectedModel;

    // Map messages to chat format
    const chatMessages = [
      ...messages.map((m) => ({
        role: m.role === 'assistant' ? 'assistant' : m.role === 'user' ? 'user' : 'system',
        content: m.content,
      })),
      { role: 'user' as const, content },
    ];

    // Start streaming via Rust proxy.
    // P1 security: API key + base URL are resolved server-side from the session's
    // provider_id — never passed from the frontend (CONTEXT.md L37/L40).
    try {
      await window.nativesAPI?.assistant?.streamChat?.({
        sessionId: activeSessionId,
        model,
        messages: chatMessages,
      });
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [activeSessionId, messages, loadMessages, selectedModel]);

  const handleStop = useCallback(async () => {
    if (activeSessionId) {
      try {
        await window.nativesAPI?.assistant?.cancelStream?.(activeSessionId);
      } catch { /* ignore */ }
    }
  }, [activeSessionId]);

  // ── Message persistence: save assistant message when stream is done ──
  useEffect(() => {
    if (streamState.done && !persistedRef.current && activeSessionId && (streamState.content || streamState.toolCall)) {
      persistedRef.current = true;
      const saveAndReload = async () => {
        try {
          // Wrap content with <think> tags if reasoning exists
          const contentToSave = streamState.reasoning
            ? `<think>${streamState.reasoning}</think>\n${streamState.content}`
            : streamState.content;

          await window.nativesAPI?.assistant?.saveMessage({
            sessionId: activeSessionId,
            role: 'assistant',
            content: contentToSave,
            status: 'done',
            tokenCount: 0,
            toolCalls: streamState.toolCall || undefined,
          });
        } catch (err) {
          toast(classifyError(err).userMessage, 'error');
        }
        await loadMessages(activeSessionId);
        // Reset stream state after persistence to prevent re-trigger
        resetStream();
      };
      saveAndReload();
    }
  }, [streamState.done, streamState.content, streamState.toolCall, streamState.reasoning, activeSessionId, loadMessages]);

  return (
    <div className="flex h-full w-full" style={{ fontFamily: 'inherit' }}>
      {/* Session list sidebar */}
      <div className="w-56 shrink-0 border-r border-[var(--border-subtle)] flex flex-col">
        <SessionList
          sessions={sessions}
          activeSessionId={activeSessionId}
          onSessionSelect={handleSessionSelect}
          onSessionDelete={handleDeleteSession}
          onNewSession={handleNewSession}
          locale={locale}
          loading={loading}
        />
      </div>

      {/* Main chat area */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Chat Header Bar */}
        <div className="shrink-0 border-b border-[var(--border-subtle)] px-4 py-2 flex items-center gap-3">
          {activeSessionId && (
            <>
              <div className="flex-1 min-w-0">
                <input
                  type="text"
                  className="w-full bg-transparent text-sm font-medium text-[var(--text)] border-none outline-none placeholder-[var(--text-disabled)]"
                  value={sessions.find(s => s.id === activeSessionId)?.title || ''}
                  placeholder={t(locale, 'aiWorkbench.sessionTitlePlaceholder')}
                  onChange={(e) => {
                    const newTitle = e.target.value;
                    setSessions((prev) => prev.map(s =>
                      s.id === activeSessionId ? { ...s, title: newTitle } : s
                    ));
                  }}
                  onBlur={(e) => {
                    const title = e.target.value.trim();
                    if (title && activeSessionId) {
                      window.nativesAPI?.assistant?.updateSessionTitle({
                        sessionId: activeSessionId,
                        title,
                      }).catch(() => {});
                    }
                  }}
                />
              </div>
              <div className="shrink-0">
                <ModelSelectorDropdown
                  providers={providers}
                  selectedProviderId={selectedProviderId}
                  selectedModel={selectedModel}
                  onSelect={handleModelSelect}
                  locale={locale}
                />
              </div>
            </>
          )}
          {noProject && activeSessionId && (
            <span className="text-[0.625rem] text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded whitespace-nowrap">
              {locale.startsWith('zh') ? '无项目上下文' : 'No Project Context'}
            </span>
          )}
        </div>

        <MessageList
          messages={messages}
          locale={locale}
          streamingContent={streamState.content}
          streamingToolCall={streamState.toolCall}
          streamingReasoning={streamState.reasoning}
          isStreaming={streamState.isStreaming}
        />
        <div ref={messagesEndRef} />
        <div className="shrink-0">
          <MessageInput
            locale={locale}
            onSend={handleSend}
            onStop={handleStop}
            isStreaming={streamState.isStreaming}
            disabled={!activeSessionId}
            noProject={noProject}
            inputDisabledReason={inputDisabledReason}
          />
        </div>
      </div>
    </div>
  );
}
