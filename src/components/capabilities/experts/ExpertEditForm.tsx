'use client';

import { useEffect, useState } from 'react';
import { Sparkles } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import {
  createCapabilityExpert,
  updateCapabilityExpert,
} from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityExpert, CapabilitySkill } from '@/types/capability';

interface ExpertEditFormProps {
  locale: Locale;
  gateway: AssistantGateway;
  /** null → create */
  expert: CapabilityExpert | null;
  /** From capability.skill.list — the data source for the skills binding. */
  skills: CapabilitySkill[];
  onClose: () => void;
  onSaved: () => void;
}

const inputStyle = {
  borderColor: 'var(--border)',
  background: 'var(--surface)',
  color: 'var(--text)',
} as const;

const PERMISSION_MODES = ['', 'readonly', 'ask', 'full_access'] as const;

/** Ceiling on an AI-generated prompt draft (same as the task tool's persona limit). */
const MAX_GENERATED_PROMPT_BYTES = 16_000;

interface DiffLine {
  kind: 'same' | 'removed' | 'added';
  text: string;
}

/** Minimal LCS line diff for the preview/diff step (no external dependency). */
function diffLines(oldText: string, newText: string): DiffLine[] {
  const a = oldText.split('\n');
  const b = newText.split('\n');
  const m = a.length;
  const n = b.length;
  const lcs: number[][] = Array.from({ length: m + 1 }, () => new Array<number>(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      // Bounds are guaranteed by the loop (i<m, j<n) and the (m+1)×(n+1) table;
      // noUncheckedIndexedAccess needs the explicit non-null assertions.
      lcs[i]![j] = a[i] === b[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < m && j < n) {
    if (a[i] === b[j]) {
      out.push({ kind: 'same', text: a[i]! });
      i++;
      j++;
    } else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) {
      out.push({ kind: 'removed', text: a[i]! });
      i++;
    } else {
      out.push({ kind: 'added', text: b[j]! });
      j++;
    }
  }
  while (i < m) {
    out.push({ kind: 'removed', text: a[i]! });
    i++;
  }
  while (j < n) {
    out.push({ kind: 'added', text: b[j]! });
    j++;
  }
  return out;
}

/** Expert form incl. multi-select skill binding (data source: capability.skill.list). */
export default function ExpertEditForm({ locale, gateway, expert, skills, onClose, onSaved }: ExpertEditFormProps) {
  const { toast } = useToast();
  const isEdit = expert !== null;
  const [name, setName] = useState(expert?.name ?? '');
  const [description, setDescription] = useState(expert?.description ?? '');
  const [systemPrompt, setSystemPrompt] = useState(expert?.systemPrompt ?? '');
  // 审计收口 #10：工具来自真实 tool.list（builtinTool.list），不再手输逗号串。
  const [tools, setTools] = useState<string[]>(expert?.tools ?? []);
  const [disallowedTools, setDisallowedTools] = useState<string[]>(expert?.disallowedTools ?? []);
  const [availableTools, setAvailableTools] = useState<string[]>([]);
  const [toolsLoading, setToolsLoading] = useState(true);
  const [toolsError, setToolsError] = useState<string | null>(null);
  const [permissionMode, setPermissionMode] = useState(expert?.permissionMode ?? '');
  const [selectedSkills, setSelectedSkills] = useState<string[]>(expert?.skills ?? []);
  const [enabled, setEnabled] = useState(expert?.enabled ?? true);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const [generateError, setGenerateError] = useState<string | null>(null);
  const [draft, setDraft] = useState<string | null>(null);

  // 真实工具列表：Host `builtin_tool_list` 是唯一来源（loading/error/retry 明确）。
  const loadTools = async () => {
    setToolsLoading(true);
    setToolsError(null);
    try {
      const list = (await window.nativesAPI?.builtinTool?.list?.()) ?? [];
      setAvailableTools(list.map((tool) => tool.id).filter(Boolean));
    } catch (cause) {
      setToolsError(classifyError(cause).userMessage);
    } finally {
      setToolsLoading(false);
    }
  };
  useEffect(() => {
    void loadTools();
  }, []);

  const toggleTool = (id: string) => {
    setTools((prev) => (prev.includes(id) ? prev.filter((s) => s !== id) : [...prev, id]));
    // allow 与 deny 互斥：同一工具不能同时出现在两个列表。
    setDisallowedTools((prev) => prev.filter((s) => s !== id));
  };
  const toggleDisallowedTool = (id: string) => {
    setDisallowedTools((prev) => (prev.includes(id) ? prev.filter((s) => s !== id) : [...prev, id]));
    setTools((prev) => prev.filter((s) => s !== id));
  };

  const toggleSkill = (id: string) => {
    setSelectedSkills((prev) => (prev.includes(id) ? prev.filter((s) => s !== id) : [...prev, id]));
  };

  const handleSave = async (promptOverride?: string) => {
    if (!name.trim()) {
      setFormError(t(locale, 'capabilities.experts.nameRequired'));
      return;
    }
    setFormError(null);
    setSubmitting(true);
    const payload: Partial<CapabilityExpert> = {
      name: name.trim(),
      description: description.trim(),
      systemPrompt: promptOverride ?? systemPrompt,
      tools,
      disallowedTools,
      permissionMode: permissionMode || null,
      skills: selectedSkills,
      // 问题10：Expert 不保存 provider/key/model —— 生成时由用户按真实可用
      // provider/model 选择，运行期 Key 经 Host credential broker 解析。
      providerId: null,
      keyId: null,
      modelId: null,
      enabled,
    };
    try {
      if (isEdit && expert) {
        await updateCapabilityExpert(gateway, expert.id, payload);
        toast(t(locale, 'capabilities.experts.saved'), 'success');
      } else {
        await createCapabilityExpert(gateway, payload);
        toast(t(locale, 'capabilities.experts.created'), 'success');
      }
      onSaved();
    } catch (e) {
      setFormError(classifyError(e).userMessage);
    } finally {
      setSubmitting(false);
    }
  };

  // ── 19.3-③: "AI 生成 Expert Prompt" reuses the ORDINARY Assistant Run ──
  // There is deliberately NO second Prompt Generator service: this starts a
  // normal run.start on a throwaway conversation through the same Assistant
  // gateway the chat uses, streams text_delta events, and hands the finished
  // draft to the preview/diff → confirm → save flow below.
  const handleAiGenerate = async () => {
    if (generating) return;
    if (!name.trim()) {
      setGenerateError(t(locale, 'capabilities.experts.nameRequired'));
      return;
    }
    setGenerating(true);
    setGenerateError(null);
    setDraft(null);
    try {
      const { readActiveProject } = await import('@/lib/active-project');
      const api =
        typeof window !== 'undefined'
          ? (window as unknown as { nativesAPI?: { db?: { get?: (k: string) => Promise<unknown> } } })
              .nativesAPI
          : undefined;
      const projectPath = await readActiveProject(api);
      if (!projectPath) {
        setGenerateError(t(locale, 'capabilities.experts.aiGenerateProjectRequired'));
        return;
      }
      // 问题10：生成模型来自真实可用 provider/model（不再是硬编码 openai/gpt-4o）。
      const { provider: providerApi } = await import('@/lib/tauri/provider');
      const providers = await providerApi.list();
      const provider = providers.find((p) => p.keys.some((k) => k.isActive)) ?? providers[0];
      if (!provider || !provider.models?.length) {
        setGenerateError(t(locale, 'capabilities.experts.aiGenerateNoModel'));
        return;
      }
      const genProvider = provider.id;
      const genModel = provider.models[0]!.id;
      const created = await gateway.request<{ id?: string }>('conversation.create', {
        mode: 'agent',
        title: t(locale, 'capabilities.experts.aiGenerateTitle'),
        provider_id: genProvider,
        model_id: genModel,
        project_id: 'config-expert-generator',
        permission_profile_id: 'readonly',
      });
      const conversationId = created?.id;
      if (!conversationId) {
        setGenerateError(t(locale, 'capabilities.experts.aiGenerateFailed'));
        return;
      }
      // 审计收口 #10：AI 生成的临时 conversation 绝不进入普通会话列表——
      // 成功/失败/取消后一律删除（零会话污染）。
      let cleanupRun: Promise<unknown> | null = null;
      try {
        const generationPrompt = [
          'You are an expert-prompt author. Write the SYSTEM PROMPT body for an AI Expert persona.',
          `Expert name: ${name.trim()}`,
          description.trim() ? `Expert description: ${description.trim()}` : '',
          'Output only the system prompt text itself — no markdown fences, no commentary, no preamble.',
        ]
          .filter(Boolean)
          .join('\n');

        const started = await gateway.request<Record<string, unknown>>('run.start', {
          conversation_id: conversationId,
          provider_id: genProvider,
          model_id: genModel,
          permission_profile: 'readonly',
          content: generationPrompt,
          project_path: projectPath,
          runtime_id: 'native',
        });
        const wire = started && typeof started === 'object' ? started : {};
        const runId = String(wire.daemon_run_id ?? wire.daemonRunId ?? wire.id ?? '');
        if (!runId) {
          setGenerateError(t(locale, 'capabilities.experts.aiGenerateFailed'));
          return;
        }
        let text = '';
        let status: string | null = null;
        for await (const event of gateway.subscribe(runId, 0)) {
          if (event.type === 'text_delta') {
            const piece = event.payload?.text;
            if (typeof piece === 'string') text += piece;
          } else if (
            event.type === 'completed' ||
            event.type === 'failed' ||
            event.type === 'cancelled' ||
            event.type === 'interrupted'
          ) {
            status = event.type;
          }
        }
        if (status !== 'completed') {
          setGenerateError(t(locale, 'capabilities.experts.aiGenerateFailed'));
          return;
        }
        const trimmed = text.trim();
        if (!trimmed) {
          setGenerateError(t(locale, 'capabilities.experts.aiGenerateFailed'));
          return;
        }
        setDraft(trimmed.slice(0, MAX_GENERATED_PROMPT_BYTES));
      } finally {
        // 无论成功/失败/取消，临时会话一律清理，不污染普通会话列表。
        cleanupRun = gateway.request('conversation.delete', { id: conversationId }).catch(() => undefined);
      }
      await cleanupRun;
    } catch (e) {
      setGenerateError(classifyError(e).userMessage);
    } finally {
      setGenerating(false);
    }
  };

  const confirmDraft = () => {
    if (draft == null) return;
    const next = draft;
    setDraft(null);
    // Confirm → save into the existing Capability Expert (same create/update
    // path as the manual form — never a second Prompt Generator service).
    void handleSave(next);
  };

  return (
    <Modal
      isOpen
      onClose={onClose}
      title={t(locale, isEdit ? 'capabilities.experts.editTitle' : 'capabilities.experts.createTitle')}
      width={560}
    >
      <div className="space-y-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.name')}
          aria-label={t(locale, 'capabilities.experts.name')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
          autoFocus
        />
        <input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.description')}
          aria-label={t(locale, 'capabilities.experts.description')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        />
        <textarea
          value={systemPrompt}
          onChange={(e) => setSystemPrompt(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.systemPrompt')}
          aria-label={t(locale, 'capabilities.experts.systemPrompt')}
          className="h-28 w-full resize-none rounded border px-3 py-2 text-sm"
          style={inputStyle}
        />
        <div className="flex items-center justify-between gap-2">
          <button
            type="button"
            onClick={() => void handleAiGenerate()}
            disabled={generating}
            className="flex items-center gap-1.5 rounded border px-2.5 py-1.5 text-xs disabled:opacity-50"
            style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-secondary)' }}
            title={t(locale, 'capabilities.experts.aiGenerate')}
          >
            <Sparkles size={13} aria-hidden />
            {t(locale, generating ? 'capabilities.common.loading' : 'capabilities.experts.aiGenerate')}
          </button>
          {generateError ? <p className="text-xs" style={{ color: 'var(--danger)' }}>{generateError}</p> : null}
        </div>
        <div className="rounded-lg border p-2.5" style={{ borderColor: 'var(--border-subtle)' }}>
          <span className="mb-1.5 block text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.experts.tools')}
          </span>
          {toolsLoading ? (
            <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>
              {t(locale, 'common.loading')}
            </p>
          ) : toolsError ? (
            <div role="alert" className="flex items-center gap-2 text-xs" style={{ color: 'var(--danger)' }}>
              <span>{toolsError}</span>
              <button type="button" className="underline" onClick={() => void loadTools()}>
                {t(locale, 'common.retry')}
              </button>
            </div>
          ) : availableTools.length === 0 ? (
            <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>
              {t(locale, 'settings.engineCapabilities.noToolsAvailable')}
            </p>
          ) : (
            <div className="space-y-2">
              {/* allow 多选（互斥：与 deny 不同时出现） */}
              <div>
                <span className="mb-1 block text-xs" style={{ color: 'var(--text-secondary)' }}>
                  {t(locale, 'settings.engineCapabilities.allowTools')}
                </span>
                <div className="flex max-h-24 flex-wrap gap-1.5 overflow-y-auto">
                  {availableTools.map((toolId) => {
                    const active = tools.includes(toolId);
                    const denied = disallowedTools.includes(toolId);
                    if (denied) return null;
                    return (
                      <button
                        key={toolId}
                        type="button"
                        role="checkbox"
                        aria-checked={active}
                        aria-label={`${t(locale, 'settings.engineCapabilities.allowTools')}: ${toolId}`}
                        onClick={() => toggleTool(toolId)}
                        className="rounded-full border px-2.5 py-1 font-mono text-xs"
                        style={{
                          borderColor: active ? 'var(--primary)' : 'var(--border-subtle)',
                          background: active ? 'var(--primary)' : 'transparent',
                          color: active ? 'var(--accent-ink)' : 'var(--text-secondary)',
                        }}
                      >
                        {toolId}
                      </button>
                    );
                  })}
                </div>
              </div>
              {/* deny 多选（互斥：与 allow 不同时出现） */}
              <div>
                <span className="mb-1 block text-xs" style={{ color: 'var(--text-secondary)' }}>
                  {t(locale, 'capabilities.experts.disallowedTools')}
                </span>
                <div className="flex max-h-24 flex-wrap gap-1.5 overflow-y-auto">
                  {availableTools.map((toolId) => {
                    const denied = disallowedTools.includes(toolId);
                    const allowed = tools.includes(toolId);
                    if (allowed) return null;
                    return (
                      <button
                        key={toolId}
                        type="button"
                        role="checkbox"
                        aria-checked={denied}
                        aria-label={`${t(locale, 'capabilities.experts.disallowedTools')}: ${toolId}`}
                        onClick={() => toggleDisallowedTool(toolId)}
                        className="rounded-full border px-2.5 py-1 font-mono text-xs"
                        style={{
                          borderColor: denied ? 'var(--danger)' : 'var(--border-subtle)',
                          background: denied ? 'var(--danger)' : 'transparent',
                          color: denied ? 'var(--accent-ink)' : 'var(--text-secondary)',
                        }}
                      >
                        {toolId} ✕
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
        </div>
        <select
          value={permissionMode ?? ''}
          onChange={(e) => setPermissionMode(e.target.value)}
          aria-label={t(locale, 'capabilities.experts.permissionMode')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        >
          {PERMISSION_MODES.map((mode) => (
            <option key={mode} value={mode}>
              {t(locale, `capabilities.experts.permissionModes.${mode === '' ? 'none' : mode}`)}
            </option>
          ))}
        </select>

        {/* Skills binding — multi-select from the capability skill store */}
        <div className="rounded-lg border p-2.5" style={{ borderColor: 'var(--border-subtle)' }}>
          <span className="mb-1.5 block text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.experts.skillsBinding')}
          </span>
          {skills.length === 0 ? (
            <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>
              {t(locale, 'capabilities.experts.noSkillsAvailable')}
            </p>
          ) : (
            <div className="flex max-h-32 flex-wrap gap-1.5 overflow-y-auto">
              {skills.map((skill) => {
                const active = selectedSkills.includes(skill.id);
                return (
                  <button
                    key={skill.id}
                    type="button"
                    role="checkbox"
                    aria-checked={active}
                    onClick={() => toggleSkill(skill.id)}
                    className="rounded-full border px-2.5 py-1 text-xs"
                    style={{
                      borderColor: active ? 'var(--primary)' : 'var(--border-subtle)',
                      background: active ? 'var(--primary)' : 'transparent',
                      color: active ? 'var(--accent-ink)' : 'var(--text-secondary)',
                    }}
                  >
                    {skill.name}
                  </button>
                );
              })}
            </div>
          )}
        </div>

        <label className="flex items-center justify-between gap-2 text-sm" style={{ color: 'var(--text)' }}>
          {t(locale, 'capabilities.experts.enabledLabel')}
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
        </label>

        <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>
          {t(locale, 'capabilities.experts.noCredentialHint')}
        </p>

        {formError ? <p className="text-sm" style={{ color: 'var(--danger)' }}>{formError}</p> : null}

        <div className="flex justify-end gap-2 pt-1">
          <button type="button" onClick={onClose} className="px-4 py-2 text-sm" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={submitting}
            className="btn btn-primary rounded px-4 py-2 text-sm disabled:opacity-50"
          >
            {t(locale, 'capabilities.common.save')}
          </button>
        </div>
      </div>

      {draft !== null && (
        <Modal
          isOpen
          onClose={() => setDraft(null)}
          title={t(locale, 'capabilities.experts.aiGenerateTitle')}
          width={720}
        >
          <div className="space-y-3">
            <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
              {t(locale, 'capabilities.experts.aiGenerateHint')}
            </p>
            <div
              className="max-h-72 overflow-y-auto rounded border font-mono text-xs"
              style={{ borderColor: 'var(--border-subtle)', background: 'var(--surface)' }}
            >
              {diffLines(systemPrompt, draft).map((line, index) => (
                <div
                  key={index}
                  className="whitespace-pre-wrap px-2 py-0.5"
                  style={{
                    background:
                      line.kind === 'added'
                        ? 'var(--success-soft)'
                        : line.kind === 'removed'
                          ? 'var(--danger-soft)'
                          : 'transparent',
                    color:
                      line.kind === 'added'
                        ? 'var(--success)'
                        : line.kind === 'removed'
                          ? 'var(--danger)'
                          : 'var(--text)',
                  }}
                >
                  {line.kind !== 'same' && (line.kind === 'added' ? '+ ' : '- ')}
                  {line.text}
                </div>
              ))}
            </div>
            <div className="flex justify-end gap-2 pt-1">
              <button
                type="button"
                onClick={() => setDraft(null)}
                className="px-4 py-2 text-sm"
                style={{ color: 'var(--text-secondary)' }}
              >
                {t(locale, 'capabilities.experts.aiGenerateCancel')}
              </button>
              <button
                type="button"
                onClick={confirmDraft}
                disabled={submitting}
                className="btn btn-primary rounded px-4 py-2 text-sm disabled:opacity-50"
              >
                {t(locale, 'capabilities.experts.aiGenerateConfirm')}
              </button>
            </div>
          </div>
        </Modal>
      )}
    </Modal>
  );
}
