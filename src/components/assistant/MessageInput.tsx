'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Check, ChevronDown, Paperclip, Plus, Send, ShieldCheck, Square, X } from 'lucide-react';
import type { Locale } from '@/i18n';
import {
  canSendAssistantDraft,
  fileNameFromPath,
  type AssistantAttachment,
  type AssistantDraft,
  type AssistantPermissionProfile,
  mimeTypeFromPath,
} from '@/lib/assistant-composer';
import ModelSelectorDropdown, { type ProviderWithModels } from './ModelSelectorDropdown';
import SlashCommandPopover from './SlashCommandPopover';
import FileMentionPopover, { type ProjectFileHit } from './FileMentionPopover';

interface SlashCommand { id: string; label: string; description: string; category: 'system' | 'skill' | 'mcp' }

interface MessageInputProps {
  locale: Locale;
  onSend: (draft: AssistantDraft) => Promise<boolean>;
  /** Cmd/Ctrl+Enter while streaming: cancel current run and send immediately. */
  onForceSend?: (draft: AssistantDraft) => Promise<boolean>;
  onStop: () => void;
  onBlockedSend?: () => void;
  isStreaming: boolean;
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
  onDraftChange?: (text: string) => void;
  /** Active project root for `@` file search. */
  projectPath?: string | null;
}

const permissionLabels = {
  readonly: { zh: '只读', en: 'Read only' },
  ask: { zh: '需要时询问', en: 'Ask when needed' },
  full_access: { zh: '完全访问', en: 'Full access' },
} as const;

export default function MessageInput(props: MessageInputProps) {
  const {
    locale, onSend, onForceSend, onStop, onBlockedSend, isStreaming,
    allowQueueWhileStreaming = false, disabled = false, inputDisabledReason = null,
    permissionProfile, onPermissionChange, providers, selectedProviderId, selectedModel, onSelectModel,
    draftText, onDraftChange, projectPath = null,
  } = props;
  const zh = locale.startsWith('zh');
  const [input, setInput] = useState(draftText ?? '');
  const [attachments, setAttachments] = useState<AssistantAttachment[]>([]);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashQuery, setSlashQuery] = useState('');
  const [mentionOpen, setMentionOpen] = useState(false);
  const [mentionQuery, setMentionQuery] = useState('');
  const [permissionOpen, setPermissionOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const lastSlashIndex = useRef(-1);
  const lastAtIndex = useRef(-1);
  const effectiveDisabled = disabled || inputDisabledReason === 'no_provider' || inputDisabledReason === 'no_model' || inputDisabledReason === 'creating';

  useEffect(() => {
    if (draftText !== undefined && draftText !== input) setInput(draftText);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- sync from store when conversation draft changes
  }, [draftText]);

  useEffect(() => {
    if (!textareaRef.current) return;
    textareaRef.current.style.height = 'auto';
    textareaRef.current.style.height = `${Math.min(Math.max(textareaRef.current.scrollHeight, 36), 200)}px`;
  }, [input]);

  const handleInputChange = (value: string) => {
    setInput(value);
    onDraftChange?.(value);
    const slashIndex = value.lastIndexOf('/');
    const atLineStart = slashIndex === 0 || (slashIndex > 0 && value.slice(0, slashIndex).endsWith('\n'));
    const afterSlash = atLineStart ? value.slice(slashIndex + 1) : '';
    const slashActive = atLineStart && !afterSlash.includes(' ') && !afterSlash.includes('\n');
    setSlashOpen(slashActive);
    if (slashActive) {
      setSlashQuery(afterSlash);
      lastSlashIndex.current = slashIndex;
    }

    // `@` file mention: last @ not followed by whitespace boundary end
    const atIndex = value.lastIndexOf('@');
    if (atIndex >= 0) {
      const before = atIndex === 0 || /[\s\n]/.test(value[atIndex - 1] ?? '');
      const fragment = value.slice(atIndex + 1);
      const valid = before && !fragment.includes(' ') && !fragment.includes('\n');
      setMentionOpen(valid && !slashActive);
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
    onDraftChange?.(next);
    setMentionOpen(false);
    textareaRef.current?.focus();
  };

  const handleSend = async (forceImmediate = false) => {
    if (submitting || effectiveDisabled || !canSendAssistantDraft(input, attachments)) return;
    if (isStreaming && !allowQueueWhileStreaming && !forceImmediate) {
      onBlockedSend?.();
      return;
    }
    const draft = { content: input.trim(), attachments };
    setSubmitting(true);
    setInput('');
    onDraftChange?.('');
    setAttachments([]);
    setSlashOpen(false);
    try {
      const sender = forceImmediate && onForceSend ? onForceSend : onSend;
      const sent = await sender(draft);
      if (!sent) {
        setInput(current => current || draft.content);
        onDraftChange?.(draft.content);
        setAttachments(current => current.length ? current : draft.attachments);
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
        const metadata = await window.nativesAPI?.fs?.readFile(path) as unknown as { size?: number } | undefined;
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
    setInput(`${input.slice(0, lastSlashIndex.current)}${command.id} `);
    setSlashOpen(false);
    textareaRef.current?.focus();
  }, [input]);

  const placeholder = inputDisabledReason === 'creating'
    ? (zh ? '正在创建会话…' : 'Starting conversation…')
    : inputDisabledReason === 'no_provider' || inputDisabledReason === 'no_model'
      ? (zh ? '请先配置可用的供应商和模型' : 'Configure a provider and model first')
      : (zh ? '描述任务，或输入 / 使用指令' : 'Describe a task, or type / for commands');

  return (
    <div className="mx-auto w-full max-w-[860px] px-5 pb-5 pt-2">
      <div className="relative rounded-[22px] border border-[var(--border)] bg-[var(--surface)] shadow-[0_8px_30px_rgba(0,0,0,0.08)]">
        <SlashCommandPopover isOpen={slashOpen} query={slashQuery} onSelect={handleSlashSelect} onClose={() => setSlashOpen(false)} disabled={false} />
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
        <textarea ref={textareaRef} value={input} onChange={event => handleInputChange(event.target.value)}
          onKeyDown={event => {
            if (event.defaultPrevented) return;
            // Shift+Enter → newline (default)
            if (event.key === 'Enter' && event.shiftKey) return;
            // Cmd/Ctrl+Enter while streaming → cancel & send now
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
              event.preventDefault();
              void handleSend(true);
              return;
            }
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault();
              void handleSend(false);
            }
          }}
          placeholder={
            isStreaming && allowQueueWhileStreaming
              ? (zh ? '运行中：Enter 入队，⌘Enter 取消并立即发送' : 'Running: Enter queues, ⌘Enter cancel & send')
              : placeholder
          }
          disabled={effectiveDisabled} rows={1}
          className="block w-full resize-none bg-transparent px-4 pb-2 pt-4 text-[15px] leading-6 text-[var(--text)] placeholder:text-[var(--text-disabled)] disabled:cursor-not-allowed"
          style={{ outline: 'none', boxShadow: 'none', overflowY: 'auto' }} />
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
          </div>
          <div className="flex min-w-0 items-center gap-1">
            <ModelSelectorDropdown providers={providers} selectedProviderId={selectedProviderId} selectedModel={selectedModel} onSelect={onSelectModel} locale={locale} />
            {isStreaming || submitting ? (
              <button type="button" onClick={onStop} title={zh ? '停止生成' : 'Stop'} className="grid h-8 w-8 place-items-center rounded-full bg-[var(--text)] text-[var(--surface)]"><Square size={12} fill="currentColor" /></button>
            ) : (
              <button type="button" onClick={() => void handleSend()} disabled={effectiveDisabled || !canSendAssistantDraft(input, attachments)} title={zh ? '发送' : 'Send'} className="grid h-8 w-8 place-items-center rounded-full bg-[var(--text)] text-[var(--surface)] transition disabled:opacity-25"><Send size={15} /></button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
