'use client';

import { useEffect, useState, useRef, useCallback } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Locale } from '@/i18n';

export interface StreamPayload {
  sessionId: string;
  delta?: string;
  toolCall?: string;
  reasoning?: string;
  done: boolean;
  error?: string;
  /** 执行引擎：工具执行状态 — pending / success / error / circuit_broken */
  toolStatus?: string;
  /** 执行引擎：工具执行结果 */
  toolResult?: unknown;
  /** 执行引擎：当前自愈失败计数 */
  selfHealCount?: number;
}

export interface UseAssistantStreamOptions {
  sessionId: string | null;
  locale: Locale;
}

export interface StreamState {
  content: string;
  toolCall: string;
  reasoning: string;
  isStreaming: boolean;
  error: string | null;
  done: boolean;
  /** 执行引擎新增 */
  toolStatus: string | null;
  toolResult: unknown;
  selfHealCount: number;
  toolEvents: Array<{ id: string; name: string; input: Record<string, unknown>; status: 'running' | 'completed' | 'failed' | 'rejected'; output?: unknown }>;
  permissionRequest: { id: string; toolName: string; reason: string; input: Record<string, unknown> } | null;
}

interface RuntimeEventPayload {
  type: string;
  tool_name?: string;
  tool_call_id?: string;
  args?: Record<string, unknown>;
  reason?: string;
  status?: string;
  output?: unknown;
}

export function useAssistantStream({ sessionId, locale }: UseAssistantStreamOptions) {
  const [streamState, setStreamState] = useState<StreamState>({
    content: '',
    toolCall: '',
    reasoning: '',
    isStreaming: false,
    error: null,
    done: false,
    toolStatus: null,
    toolResult: null,
    selfHealCount: 0,
    toolEvents: [],
    permissionRequest: null,
  });
  const bufferRef = useRef('');
  const toolCallBufferRef = useRef('');
  const reasoningBufferRef = useRef('');
  const unlistenRef = useRef<(() => void) | null>(null);
  const activeRef = useRef(true);

  useEffect(() => {
    activeRef.current = true;

    if (!sessionId) {
      setStreamState({
        content: '', toolCall: '', reasoning: '', isStreaming: false,
        error: null, done: false, toolStatus: null, toolResult: null, selfHealCount: 0,
        toolEvents: [], permissionRequest: null,
      });
      return;
    }

    const setupListener = async () => {
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }

      const unlisten = await listen<StreamPayload>('assistant:stream_update', (event) => {
        if (!activeRef.current) return;
        if (event.payload.sessionId !== sessionId) return;

        const { delta, toolCall, reasoning, done, error, toolStatus, toolResult, selfHealCount } = event.payload;

        if (error) {
          setStreamState((prev) => ({
            ...prev,
            isStreaming: false,
            error,
            done: true,
            toolStatus: toolStatus ?? prev.toolStatus,
            selfHealCount: selfHealCount ?? prev.selfHealCount,
          }));
          return;
        }

        // Accumulate content delta
        if (delta) {
          const thinkRe = /^<think>([\s\S]*?)<\/think>\s*/;
          const thinkMatch = delta.match(thinkRe);
          if (thinkMatch && !reasoning) {
            reasoningBufferRef.current += thinkMatch[1];
            setStreamState((prev) => ({
              ...prev,
              reasoning: reasoningBufferRef.current,
              isStreaming: !done,
            }));
            const afterThink = delta.slice(thinkMatch[0].length);
            if (afterThink) {
              bufferRef.current += afterThink;
            }
          } else {
            bufferRef.current += delta;
          }
          setStreamState((prev) => ({
            ...prev,
            content: bufferRef.current,
            isStreaming: !done,
          }));
        }

        // Accumulate toolCall delta
        if (toolCall) {
          toolCallBufferRef.current += toolCall;
          setStreamState((prev) => ({
            ...prev,
            toolCall: toolCallBufferRef.current,
            isStreaming: !done,
          }));
        }

        // Accumulate reasoning delta
        if (reasoning) {
          reasoningBufferRef.current += reasoning;
          setStreamState((prev) => ({
            ...prev,
            reasoning: reasoningBufferRef.current,
            isStreaming: !done,
          }));
        }

        // 执行引擎状态更新
        if (toolStatus || toolResult !== undefined || selfHealCount !== undefined) {
          setStreamState((prev) => ({
            ...prev,
            ...(toolStatus ? { toolStatus } : {}),
            ...(toolResult !== undefined ? { toolResult } : {}),
            ...(selfHealCount !== undefined ? { selfHealCount } : {}),
          }));
        }

        if (done) {
          setStreamState((prev) => ({
            ...prev,
            isStreaming: false,
            done: true,
          }));
        }
      });

      unlistenRef.current = unlisten;

      const unlistenRuntime = await listen<RuntimeEventPayload>(`assistant://stream/${sessionId}`, event => {
        const payload = event.payload;
        if (payload.type === 'tool_started' && payload.tool_call_id) {
          setStreamState(previous => ({ ...previous, toolEvents: [...previous.toolEvents.filter(item => item.id !== payload.tool_call_id), {
            id: payload.tool_call_id!, name: payload.tool_name ?? 'tool', input: payload.args ?? {}, status: 'running',
          }] }));
        } else if ((payload.type === 'tool_completed' || payload.type === 'tool_rejected') && payload.tool_call_id) {
          setStreamState(previous => ({ ...previous, toolEvents: previous.toolEvents.map(item => item.id === payload.tool_call_id ? {
            ...item, status: payload.type === 'tool_rejected' ? 'rejected' : payload.status === 'success' ? 'completed' : 'failed', output: payload.output ?? payload.reason,
          } : item) }));
        } else if (payload.type === 'permission_requested' && payload.tool_call_id) {
          setStreamState(previous => ({ ...previous, permissionRequest: { id: payload.tool_call_id!, toolName: payload.tool_name ?? 'tool', reason: payload.reason ?? '', input: payload.args ?? {} } }));
        }
      });
      const previousUnlisten = unlistenRef.current;
      unlistenRef.current = () => { previousUnlisten?.(); unlistenRuntime(); };
    };

    setupListener();

    return () => {
      activeRef.current = false;
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    };
  }, [sessionId]);

  const resetStream = useCallback(() => {
    bufferRef.current = '';
    toolCallBufferRef.current = '';
    reasoningBufferRef.current = '';
    setStreamState({
      content: '', toolCall: '', reasoning: '', isStreaming: false,
      error: null, done: false, toolStatus: null, toolResult: null, selfHealCount: 0,
      toolEvents: [], permissionRequest: null,
    });
  }, []);

  return { streamState, resetStream };
}
