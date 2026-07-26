'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { Blocks, Bot, Check, ChevronDown, Paperclip, Plus, Send, ShieldCheck, Square, X } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import {
  canSendAssistantDraft,
  fileNameFromPath,
  type AssistantAttachment,
  type AssistantDraft,
  type AssistantPermissionProfile,
  mimeTypeFromPath,
} from '@/lib/assistant-composer';
import {
  detectSlashInput,
  filterSlashCommands,
  listSlashCommands,
  nextSlashIndex,
  type SlashCommand,
} from '@/lib/assistant-slash';
import {
  loadPersistedQuestionHistory,
  savePersistedQuestionHistory,
} from '@/lib/assistant-workspace/persistence';
import { fsApi } from '@/lib/files-api';
import ModelSelectorDropdown, { type ProviderWithModels } from './ModelSelectorDropdown';
import SlashCommandPopover from './SlashCommandPopover';
import FileMentionPopover, { type ProjectFileHit } from './FileMentionPopover';

export interface ComposerSubagent {
  id: string;
  name: string;
  status?: string;
}

interface MessageInputProps {
  locale: Locale;
  onSend: (draft: AssistantDraft) => Promise<boolean>;
  /** Cmd/Ctrl+Enter while streaming: cancel current run and send immediately. */
  onForceSend?: (draft: AssistantDraft) => Promise<boolean>;
  /**
   * When advertised and run is active: Cmd/Ctrl+Enter injects into the current run
   * (promptQueue.interject) instead of cancel-and-send. Enter still queues.
   */
  onInterject?: (content: string) => Promise<boolean> | boolean;
  onStop: () => void;
  onBlockedSend?: () => void;
  isStreaming: boolean;
  isStopping?: boolean;
  /** When true, Enter while streaming queues via onSend instead of blocking. */
  allowQueueWhileStreaming?: boolean;
  disabled?: boolean;
  inputDisabledReason?: 'no_provider' | 'no_model' | 'creating' | null;
  permissionProfile: AssistantPermissionProfile;
  onPermissionChange: (profile: AssistantPermissionProfile) => Promise<void>;
  providers: ProviderWithModels[];
  selectedProviderId: string;
  selectedModel?: string;
  onSelectModel: (providerId: string, model: string) => void;
  /** Controlled draft text from workspace store (per-conversation). */
  draftText?: string;
  /**
   * Persist draft to the workspace store. Called debounced while typing so
   * each keystroke does not re-render the whole Workbench. Second arg is the
   * conversation id captured at edit time (survives switch during debounce).
   * Empty string / unmount / draftKey change flush immediately.
   */
  onDraftChange?: (text: string, conversationId?: string | null) => void;
  /** Conversation id for this draft — used as debounce key + flush identity. */
  draftKey?: string | null;
  /** Active project root for `@` file search. */
  projectPath?: string | null;
  /** Child agents of the root conversation; clicking one opens its hidden session. */
  subagents?: ComposerSubagent[];
  activeSubagent?: ComposerSubagent | null;
  onSelectSubagent?: (id: string) => void;
  changeSummary?: { fileCount: number; additions: number; deletions: number } | null;
  /**
   * ADR-0016 capability picker (gated by conversation.updateCapabilities).
   * Presence of the toggle renders the「能力」button; the popover itself is a
   * controlled slot so the Workbench can lazy-load it off the initial bundle.
   */
  onToggleCapabilities?: () => void;
  capabilityCount?: number;
  capabilityPickerSlot?: ReactNode;
}

/** Idle ms before pushing draft text into the workspace store. */
const DRAFT_PERSIST_DEBOUNCE_MS = 200;

const permissionLabels = {
  readonly: { zh: '只读', en: 'Read only' },
  ask: { zh: '需要时询问', en: 'Ask when needed' },
  full_access: { zh: '完全访问', en: 'Full access' },
} as const;

const AGENT_ACCENTS = [
  { solid: '#2563eb', soft: 'rgba(37,99,235,.12)', border: 'rgba(37,99,235,.38)' },
  { solid: '#7c3aed', soft: 'rgba(124,58,237,.12)', border: 'rgba(124,58,237,.38)' },
  { solid: '#0891b2', soft: 'rgba(8,145,178,.12)', border: 'rgba(8,145,178,.38)' },
  { solid: '#c2410c', soft: 'rgba(194,65,12,.12)', border: 'rgba(194,65,12,.38)' },
  { solid: '#be185d', soft: 'rgba(190,24,93,.12)', border: 'rgba(190,24,93,.38)' },
];

function agentAccent(id: string) {
  let hash = 0;
  for (let index = 0; index < id.length; index++) hash = (hash * 31 + id.charCodeAt(index)) | 0;
  return AGENT_ACCENTS[Math.abs(hash) % AGENT_ACCENTS.length]!;
}

export default function MessageInput(props: MessageInputProps) {
  const {
    locale, onSend, onForceSend, onInterject, onStop, onBlockedSend, isStreaming, isStopping = false,
    allowQueueWhileStreaming = false, disabled = false, inputDisabledReason = null,
    permissionProfile, onPermissionChange, providers, selectedProviderId, selectedModel, onSelectModel,
    draftText, onDraftChange, draftKey = null, projectPath = null,
    subagents = [], activeSubagent = null, onSelectSubagent, changeSummary = null,
    onToggleCapabilities, capabilityCount = 0, capabilityPickerSlot = null,
  } = props;
  const zh = locale.startsWith('zh');
  const [input, setInput] = useState(draftText ?? '');
  const [attachments, setAttachments] = useState<AssistantAttachment[]>([]);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashQuery, setSlashQuery] = useState('');
  const [slashSelectedIndex, setSlashSelectedIndex] = useState(0);
  const [mentionOpen, setMentionOpen] = useState(false);
  const [mentionQuery, setMentionQuery] = useState('');
  const [permissionOpen, setPermissionOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [questionHistory, setQuestionHistory] = useState<string[]>(() => loadPersistedQuestionHistory(projectPath));
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const composingRef = useRef(false);
  const lastSlashIndex = useRef(-1);
  const lastAtIndex = useRef(-1);
  const onDraftChangeRef = useRef(onDraftChange);
  onDraftChangeRef.current = onDraftChange;
  const draftKeyRef = useRef(draftKey);
  draftKeyRef.current = draftKey;
  /** Pending store write: key = conversation at edit time (survives switch). */
  const pendingDraftRef = useRef<{ key: string | null; text: string } | null>(null);
  const draftTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** True while local input is ahead of the last store draftText we adopted. */
  const localDirtyRef = useRef(false);
  const questionHistoryIndexRef = useRef(questionHistory.length);
  const questionHistoryDraftRef = useRef(input);
  const effectiveDisabled = disabled || inputDisabledReason === 'no_provider' || inputDisabledReason === 'no_model' || inputDisabledReason === 'creating';
  const activeAccent = activeSubagent ? agentAccent(activeSubagent.id) : null;

  // Native currently exposes no slash commands — empty list, honest empty state.
  const availableCommands = useMemo(() => listSlashCommands(), []);
  const filteredCommands = useMemo(
    () => filterSlashCommands(availableCommands, slashQuery),
    [availableCommands, slashQuery],
  );

  const flushDraftToStore = useCallback(() => {
    if (draftTimerRef.current != null) {
      clearTimeout(draftTimerRef.current);
      draftTimerRef.current = null;
    }
    const pending = pendingDraftRef.current;
    if (!pending) return;
    pendingDraftRef.current = null;
    onDraftChangeRef.current?.(pending.text, pending.key);
  }, []);

  const scheduleDraftToStore = useCallback(
    (value: string, options?: { immediate?: boolean }) => {
      localDirtyRef.current = true;
      pendingDraftRef.current = { key: draftKeyRef.current, text: value };
      if (options?.immediate) {
        flushDraftToStore();
        return;
      }
      if (draftTimerRef.current != null) clearTimeout(draftTimerRef.current);
      draftTimerRef.current = setTimeout(() => {
        draftTimerRef.current = null;
        flushDraftToStore();
      }, DRAFT_PERSIST_DEBOUNCE_MS);
    },
    [flushDraftToStore],
  );

  // Always flush on unmount so a remount (settings round-trip) restores text.
  useEffect(() => () => flushDraftToStore(), [flushDraftToStore]);

  // Conversation switch: flush the previous key's pending text, then adopt store draft.
  useEffect(() => {
    flushDraftToStore();
    localDirtyRef.current = false;
    if (draftText !== undefined) setInput(draftText);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when conversation (draftKey) changes
  }, [draftKey]);

  // Store → input for the *same* conversation only when we are not mid-edit
  // (e.g. send cleared store to ''). Never clobber newer local keystrokes with
  // a lagging draftText from a previous debounce flush.
  useEffect(() => {
    if (draftText === undefined) return;
    if (localDirtyRef.current) {
      // Catch up: store finally matches what we typed → clear dirty.
      if (draftText === pendingDraftRef.current?.text || draftText === input) {
        localDirtyRef.current = false;
        pendingDraftRef.current = null;
      }
      return;
    }
    if (draftText !== input) setInput(draftText);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- input is local authority while dirty
  }, [draftText]);

  useEffect(() => {
    const history = loadPersistedQuestionHistory(projectPath);
    setQuestionHistory(history);
    questionHistoryIndexRef.current = history.length;
    questionHistoryDraftRef.current = input;
    // eslint-disable-next-line react-hooks/exhaustive-deps -- project change resets its history cursor
  }, [projectPath]);

  useEffect(() => {
    if (!textareaRef.current) return;
    textareaRef.current.style.height = 'auto';
    textareaRef.current.style.height = `${Math.min(Math.max(textareaRef.current.scrollHeight, 36), 200)}px`;
  }, [input]);

  useEffect(() => {
    setSlashSelectedIndex(0);
  }, [slashQuery, slashOpen]);

  const closeSlashMenu = useCallback(() => {
    setSlashOpen(false);
    setSlashQuery('');
    lastSlashIndex.current = -1;
  }, []);

  const syncSlashFromValue = useCallback((value: string, caret?: number) => {
    const detection = detectSlashInput(value, caret);
    setSlashOpen(detection.active);
    if (detection.active) {
      setSlashQuery(detection.query);
      lastSlashIndex.current = detection.slashIndex;
    } else {
      setSlashQuery('');
      lastSlashIndex.current = -1;
    }
    return detection;
  }, []);

  const handleInputChange = (value: string) => {
    setInput(value);
    // Local state updates immediately; store (and Workbench re-render) waits.
    scheduleDraftToStore(value);

    const caret = textareaRef.current?.selectionStart;
    const detection = syncSlashFromValue(value, caret);

    // `@` file mention: last @ not followed by whitespace boundary end
    const atIndex = value.lastIndexOf('@');
    if (atIndex >= 0) {
      const before = atIndex === 0 || /[\s\n]/.test(value[atIndex - 1] ?? '');
      const fragment = value.slice(atIndex + 1);
      const valid = before && !fragment.includes(' ') && !fragment.includes('\n');
      setMentionOpen(valid && !detection.active);
      if (valid) {
        setMentionQuery(fragment);
        lastAtIndex.current = atIndex;
      }
    } else {
      setMentionOpen(false);
      setMentionQuery('');
    }
  };

  const handleMentionSelect = (file: ProjectFileHit) => {
    const before = input.slice(0, lastAtIndex.current);
    const afterCursor = input.slice(lastAtIndex.current + 1 + mentionQuery.length);
    const next = `${before}@${file.path} ${afterCursor}`;
    setInput(next);
    scheduleDraftToStore(next);
    setMentionOpen(false);
    textareaRef.current?.focus();
  };

  const handleSend = async (forceImmediate = false) => {
    if (submitting || effectiveDisabled || !canSendAssistantDraft(input, attachments)) return;
    if (isStreaming && !allowQueueWhileStreaming && !forceImmediate) {
      onBlockedSend?.();
      return;
    }
    // Free-form content (including `/anything`) goes through ordinary send unchanged.
    const draft = { content: input.trim(), attachments };
    setSubmitting(true);
    setInput('');
    // Immediate so store is empty before send path / remount races.
    scheduleDraftToStore('', { immediate: true });
    setAttachments([]);
    closeSlashMenu();
    try {
      // Prefer interject into the active run when capability is wired (Cmd/Ctrl+Enter).
      if (forceImmediate && isStreaming && onInterject && draft.content) {
        const ok = await onInterject(draft.content);
        if (!ok) {
          setInput((current) => current || draft.content);
          scheduleDraftToStore(draft.content, { immediate: true });
          setAttachments((current) => (current.length ? current : draft.attachments));
        } else {
          const history = savePersistedQuestionHistory(projectPath, draft.content);
          setQuestionHistory(history);
          questionHistoryIndexRef.current = history.length;
        }
        return;
      }
      const sender = forceImmediate && onForceSend ? onForceSend : onSend;
      const sent = await sender(draft);
      if (!sent) {
        setInput(current => current || draft.content);
        scheduleDraftToStore(draft.content, { immediate: true });
        setAttachments(current => current.length ? current : draft.attachments);
      } else {
        const history = savePersistedQuestionHistory(projectPath, draft.content);
        setQuestionHistory(history);
        questionHistoryIndexRef.current = history.length;
      }
    } finally {
      setSubmitting(false);
    }
  };

  const handleAddFiles = async () => {
    const dialog = window.nativesAPI?.dialog;
    if (!dialog?.pickFiles) {
      // Browser / fixture shell has no native dialog — surface instead of silent no-op.
      return;
    }
    let paths: string[] = [];
    try {
      paths = (await dialog.pickFiles()) ?? [];
    } catch {
      return;
    }
    if (!paths.length) return;
    const selected = (await Promise.all(paths.map(async path => {
      try {
        // files-api 契约：fs 不可用时抛 FilesApiUnavailableError，由下方 catch 降级为 size 0（与原可选链语义等价）
        const metadata = await fsApi().readFile(path) as unknown as { size?: number } | undefined;
        return {
          path,
          name: fileNameFromPath(path),
          mimeType: mimeTypeFromPath(path),
          size: Number(metadata?.size ?? 0),
        };
      } catch {
        // Still attach with size 0 if metadata read fails — host will re-validate.
        return {
          path,
          name: fileNameFromPath(path),
          mimeType: mimeTypeFromPath(path),
          size: 0,
        };
      }
    }))).filter((file): file is AssistantAttachment => file !== null);
    setAttachments(current => {
      const known = new Set(current.map(item => item.path));
      return [...current, ...selected.filter(file => !known.has(file.path))];
    });
  };

  const handleSlashSelect = useCallback((command: SlashCommand) => {
    const prefix = input.slice(0, lastSlashIndex.current);
    const afterQuery = input.slice(lastSlashIndex.current + 1 + slashQuery.length);
    const next = `${prefix}${command.id} ${afterQuery}`;
    setInput(next);
    scheduleDraftToStore(next);
    closeSlashMenu();
    textareaRef.current?.focus();
  }, [input, slashQuery, scheduleDraftToStore, closeSlashMenu]);

  const handleTextareaKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.defaultPrevented) return;
    // Chromium reports the Enter that confirms some IME candidates after
    // compositionend, with isComposing already false but keyCode still 229.
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229 || event.key === 'Process' || composingRef.current) return;

    // Slash menu keyboard ownership (no document listener).
    if (slashOpen) {
      if (event.key === 'Escape') {
        event.preventDefault();
        // Close menu; keep text as-is so `/abc` can still be sent later.
        closeSlashMenu();
        return;
      }
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        if (filteredCommands.length > 0) {
          setSlashSelectedIndex((prev) => nextSlashIndex(prev, filteredCommands.length, 1));
        }
        return;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        if (filteredCommands.length > 0) {
          setSlashSelectedIndex((prev) => nextSlashIndex(prev, filteredCommands.length, -1));
        }
        return;
      }
      if (event.key === 'Enter' && !event.shiftKey && !(event.metaKey || event.ctrlKey)) {
        // Menu open: never send. Select if a command is highlighted; else no-op.
        event.preventDefault();
        const selected = filteredCommands[slashSelectedIndex];
        if (selected) handleSlashSelect(selected);
        return;
      }
      // Cmd/Ctrl+Enter while menu open still force-sends (existing force-send contract).
      if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        closeSlashMenu();
        void handleSend(true);
        return;
      }
      // Shift+Enter falls through → newline (default).
    }

    if ((event.key === 'ArrowUp' || event.key === 'ArrowDown') && questionHistory.length > 0) {
      event.preventDefault();
      const currentIndex = questionHistoryIndexRef.current;
      if (currentIndex === questionHistory.length) questionHistoryDraftRef.current = input;
      const nextIndex = Math.max(0, Math.min(questionHistory.length, currentIndex + (event.key === 'ArrowUp' ? -1 : 1)));
      const next = nextIndex === questionHistory.length ? questionHistoryDraftRef.current : questionHistory[nextIndex]!;
      questionHistoryIndexRef.current = nextIndex;
      setInput(next);
      scheduleDraftToStore(next, { immediate: true });
      syncSlashFromValue(next);
      return;
    }

    // Shift+Enter → newline (default)
    if (event.key === 'Enter' && event.shiftKey) return;

    // Cmd/Ctrl+Enter → cancel & send now (or send immediately when idle)
    if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void handleSend(true);
      return;
    }

    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      void handleSend(false);
    }
  };

  const placeholder = inputDisabledReason === 'creating'
    ? (zh ? '正在创建会话…' : 'Starting conversation…')
    : inputDisabledReason === 'no_provider' || inputDisabledReason === 'no_model'
      ? (zh ? '请先配置可用的供应商和模型' : 'Configure a provider and model first')
      : (zh ? '描述任务，或输入 / 使用指令' : 'Describe a task, or type / for commands');

  return (
    <div className="mx-auto w-full max-w-[860px] px-5 pb-5 pt-2">
      {(subagents.length > 0 || Boolean(changeSummary && changeSummary.fileCount > 0)) && (
        <div className="mb-2 flex flex-wrap items-center gap-2">
          {subagents.map((agent) => {
            const accent = agentAccent(agent.id);
            const selected = activeSubagent?.id === agent.id;
            return (
              <button
                key={agent.id}
                type="button"
                onClick={() => onSelectSubagent?.(agent.id)}
                className="flex items-center gap-1.5 rounded-full border px-2 py-1 text-xs transition hover:brightness-95"
                style={{
                  color: accent.solid,
                  backgroundColor: selected ? accent.soft : 'var(--surface)',
                  borderColor: selected ? accent.border : 'var(--border-subtle)',
                }}
                aria-pressed={selected}
                title={zh ? `进入 ${agent.name} 的会话` : `Open ${agent.name}'s conversation`}
              >
                <span className="grid h-5 w-5 place-items-center rounded-full text-[10px] font-semibold text-white" style={{ backgroundColor: accent.solid }} aria-hidden>{agent.name.slice(0, 1).toUpperCase()}</span>
                <span className="max-w-32 truncate">{agent.name}</span>
              </button>
            );
          })}
          {changeSummary && changeSummary.fileCount > 0 ? (
            <span className="ml-auto flex items-center gap-1.5 text-xs text-[var(--text-secondary)]" title={zh ? '本次对话文件变更' : 'Changes in this conversation'}>
              <Bot size={14} className="text-[var(--text-disabled)]" />
              <span>{zh ? `${changeSummary.fileCount} 个文件` : `${changeSummary.fileCount} files`}</span>
              <span className="font-medium text-emerald-500">+{changeSummary.additions}</span>
              <span className="font-medium text-red-500">−{changeSummary.deletions}</span>
            </span>
          ) : null}
        </div>
      )}
      <div className="relative rounded-[22px] border bg-[var(--surface)] shadow-[0_8px_30px_rgba(0,0,0,0.08)]" style={activeAccent ? { borderColor: activeAccent.border, boxShadow: `0 8px 30px ${activeAccent.soft}` } : undefined}>
        <SlashCommandPopover
          isOpen={slashOpen}
          query={slashQuery}
          commands={filteredCommands}
          selectedIndex={slashSelectedIndex}
          onSelect={handleSlashSelect}
          onHoverIndex={setSlashSelectedIndex}
          onClose={closeSlashMenu}
          emptyMessage={t(locale, 'assistant.slashEmpty')}
          headerLabel={t(locale, 'assistant.slashHeader')}
        />
        <FileMentionPopover
          open={mentionOpen}
          query={mentionQuery}
          projectPath={projectPath}
          locale={locale}
          onSelect={handleMentionSelect}
          onClose={() => setMentionOpen(false)}
        />
        {attachments.length > 0 && (
          <div className="flex flex-wrap gap-2 px-4 pt-3">
            {attachments.map(file => (
              <div key={file.path} title={file.path} className="flex max-w-52 items-center gap-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)] px-2 py-1 text-xs text-[var(--text-secondary)]">
                <Paperclip size={12} /><span className="truncate">{file.name}</span>
                <button type="button" onClick={() => setAttachments(items => items.filter(item => item.path !== file.path))} className="rounded p-0.5 hover:bg-[var(--surface-active)]" aria-label={zh ? '移除附件' : 'Remove attachment'}><X size={11} /></button>
              </div>
            ))}
          </div>
        )}
        {activeSubagent && activeAccent && (
          <div className="px-4 pt-3">
            <span className="inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs font-medium" style={{ color: activeAccent.solid, backgroundColor: activeAccent.soft, borderColor: activeAccent.border }}>
              <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: activeAccent.solid }} />
              {activeSubagent.name}
            </span>
          </div>
        )}
        <textarea
          ref={textareaRef}
          value={input}
          onChange={event => handleInputChange(event.target.value)}
          onKeyDown={handleTextareaKeyDown}
          onCompositionStart={() => { composingRef.current = true; }}
          onCompositionEnd={() => { composingRef.current = false; }}
          placeholder={
            isStreaming && allowQueueWhileStreaming
              ? onInterject
                ? (zh
                    ? '运行中：Enter 入队，⌘Enter 插话到当前运行'
                    : 'Running: Enter queues, ⌘Enter interjects into run')
                : (zh ? '运行中：Enter 入队，⌘Enter 取消并立即发送' : 'Running: Enter queues, ⌘Enter cancel & send')
              : placeholder
          }
          disabled={effectiveDisabled}
          rows={1}
          className={`block w-full resize-none bg-transparent px-4 pb-2 ${activeSubagent ? 'pt-2' : 'pt-4'} text-[15px] leading-6 text-[var(--text)] placeholder:text-[var(--text-disabled)] disabled:cursor-not-allowed`}
          style={{ outline: 'none', boxShadow: 'none', overflowY: 'auto' }}
        />
        <div className="flex items-center justify-between gap-3 px-3 pb-3">
          <div className="flex min-w-0 items-center gap-1">
            <button
              type="button"
              onClick={() => void handleAddFiles()}
              disabled={effectiveDisabled}
              title={zh ? '添加附件' : 'Add attachment'}
              className="grid h-8 w-8 place-items-center rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-40"
            >
              <Plus size={18} />
            </button>
            <div className="relative">
              <button
                type="button"
                onClick={() => setPermissionOpen(open => !open)}
                disabled={effectiveDisabled}
                title={zh ? '权限配置' : 'Permission profile'}
                className="flex h-8 items-center gap-1.5 rounded-lg px-2 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-40"
              >
                <ShieldCheck size={15} />
                <span>{permissionLabels[permissionProfile][zh ? 'zh' : 'en']}</span>
                <ChevronDown size={12} />
              </button>
              {permissionOpen && !effectiveDisabled && (
                <div className="absolute bottom-full left-0 z-50 mb-2 w-48 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-1.5 shadow-popup">
                  {(Object.keys(permissionLabels) as AssistantPermissionProfile[]).map(profile => (
                    <button
                      key={profile}
                      type="button"
                      onClick={() => {
                        setPermissionOpen(false);
                        void onPermissionChange(profile);
                      }}
                      className="flex w-full items-center justify-between rounded-lg px-2.5 py-2 text-left text-sm text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
                    >
                      {permissionLabels[profile][zh ? 'zh' : 'en']}
                      {permissionProfile === profile && <Check size={14} />}
                    </button>
                  ))}
                </div>
              )}
            </div>
            {onToggleCapabilities && (
              <div className="relative">
                <button
                  type="button"
                  onClick={onToggleCapabilities}
                  disabled={effectiveDisabled}
                  title={t(locale, 'capabilities.picker.title')}
                  aria-label={t(locale, 'capabilities.picker.title')}
                  className="flex h-8 items-center gap-1.5 rounded-lg px-2 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-40"
                >
                  <Blocks size={15} />
                  <span>{t(locale, 'capabilities.picker.open')}</span>
                  {capabilityCount > 0 && (
                    <span
                      className="grid h-4 min-w-4 place-items-center rounded-full px-1 text-[10px] font-semibold"
                      style={{ background: 'var(--primary)', color: '#fff' }}
                    >
                      {capabilityCount}
                    </span>
                  )}
                </button>
                {capabilityPickerSlot}
              </div>
            )}
          </div>
          <div className="flex min-w-0 items-center gap-1">
            <ModelSelectorDropdown providers={providers} selectedProviderId={selectedProviderId} selectedModel={selectedModel} onSelect={onSelectModel} locale={locale} />
            {isStreaming || submitting ? (
              <button type="button" onClick={onStop} disabled={isStopping} title={isStopping ? (zh ? '停止中…' : 'Stopping…') : (zh ? '停止生成' : 'Stop')} className="grid h-8 w-8 place-items-center rounded-full bg-[var(--text)] text-[var(--surface)] disabled:opacity-50"><Square size={12} fill="currentColor" /></button>
            ) : (
              <button type="button" onClick={() => void handleSend()} disabled={effectiveDisabled || !canSendAssistantDraft(input, attachments)} title={zh ? '发送' : 'Send'} className="grid h-8 w-8 place-items-center rounded-full bg-[var(--text)] text-[var(--surface)] transition disabled:opacity-25"><Send size={15} /></button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
