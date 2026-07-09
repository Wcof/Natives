'use client';

import { useCallback, useEffect, useState } from 'react';
import { useLocale, t as tr } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState, ErrorState } from '@/components/ui/EmptyState';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import type { UserProvider } from '@/types/provider';

interface Subagent {
  id: string; name: string; role: string; instructions: string; tools: string;
  providerId: string | null; providerKeyId: string | null; modelId: string;
  fallbackEnabled: boolean; maxRuns: number; enabled: boolean;
  createdAt: string; updatedAt: string;
}
interface SubagentRun {
  id: string; subagentId: string; status: string;
  inputText: string; outputText: string; errorText: string;
  startedAt: string; finishedAt: string | null;
  providerUsed: string; keyLabel: string;
}

const MAX_ERROR_LENGTH = 500;

export default function SubagentsPage() {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();

  const [agents, setAgents] = useState<Subagent[]>([]);
  const [providers, setProviders] = useState<UserProvider[]>([]);
  const [selected, setSelected] = useState<Subagent | null>(null);
  const [runs, setRuns] = useState<SubagentRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState(false);
  const [running, setRunning] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  // Form state
  const [formName, setFormName] = useState('');
  const [formRole, setFormRole] = useState('');
  const [formInstructions, setFormInstructions] = useState('');
  const [formTools, setFormTools] = useState('');
  const [formProviderId, setFormProviderId] = useState('');
  const [formProviderKeyId, setFormProviderKeyId] = useState('');
  const [formModelId, setFormModelId] = useState('');
  const [formFallbackEnabled, setFormFallbackEnabled] = useState(true);
  const [formMaxRuns, setFormMaxRuns] = useState(10);
  const [runInput, setRunInput] = useState('');
  const [runResult, setRunResult] = useState<string | null>(null);

  const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;

  const loadData = useCallback(async () => {
    if (!api?.subagent) {
      setLoadError(t('subagent.loadFailed'));
      setLoading(false);
      return;
    }
    setLoading(true);
    setLoadError(null);
    try {
      const [subagentRes, providerRes] = await Promise.all([
        api.subagent.list(),
        api.provider?.list?.() ?? Promise.resolve([]),
      ]);
      const providerList = Array.isArray(providerRes) ? providerRes as UserProvider[] : [];
      setProviders(providerList);
      setAgents(subagentRes as Subagent[]);
    } catch (e) {
      const classified = classifyError(e);
      setLoadError(classified.userMessage);
    } finally { setLoading(false); }
  }, [api, t]);

  useEffect(() => { loadData(); }, [loadData]);

  const loadRuns = useCallback(async (agentId: string) => {
    if (!api?.subagent) return;
    try {
      const res = await api.subagent.listRuns(agentId);
      setRuns(res as SubagentRun[]);
    } catch (e) {
      const classified = classifyError(e);
      toast(classified.userMessage, 'error');
    }
  }, [api, toast]);

  const handleSelect = (agent: Subagent) => {
    setSelected(agent);
    setEditing(false);
    setRunning(false);
    setRunInput('');
    setRunResult(null);
    setSaveError(null);
    loadRuns(agent.id);
  };

  const handleCreate = async () => {
    if (!api?.subagent || !formName.trim()) return;
    if (!formProviderId || !formProviderKeyId || !formModelId.trim()) {
      setSaveError(t('subagent.bindingRequired'));
      return;
    }
    setSaveError(null);
    try {
      await api.subagent.create({
        name: formName.trim(), role: formRole, instructions: formInstructions,
        tools: formTools, providerId: formProviderId, providerKeyId: formProviderKeyId,
        modelId: formModelId.trim(), fallbackEnabled: formFallbackEnabled, maxRuns: formMaxRuns,
      });
      setCreating(false);
      resetForm();
      toast(t('subagent.created'), 'success');
      loadData();
    } catch (e) {
      const classified = classifyError(e);
      setSaveError(classified.userMessage);
    }
  };

  const handleUpdate = async () => {
    if (!api?.subagent || !selected || !formName.trim()) return;
    if (!formProviderId || !formProviderKeyId || !formModelId.trim()) {
      setSaveError(t('subagent.bindingRequired'));
      return;
    }
    setSaveError(null);
    try {
      await api.subagent.update({
        id: selected.id, name: formName.trim(), role: formRole,
        instructions: formInstructions, tools: formTools,
        providerId: formProviderId, providerKeyId: formProviderKeyId,
        modelId: formModelId.trim(), enabled: selected.enabled,
        fallbackEnabled: formFallbackEnabled, maxRuns: formMaxRuns,
      });
      setEditing(false);
      setSelected({ ...selected, name: formName.trim(), role: formRole,
        instructions: formInstructions, tools: formTools,
        providerId: formProviderId, providerKeyId: formProviderKeyId,
        modelId: formModelId.trim(), fallbackEnabled: formFallbackEnabled, maxRuns: formMaxRuns });
      toast(t('subagent.updated'), 'success');
      loadData();
    } catch (e) {
      const classified = classifyError(e);
      setSaveError(classified.userMessage);
    }
  };

  const handleDelete = async () => {
    if (!api?.subagent || !confirmDelete) return;
    try {
      await api.subagent.delete(confirmDelete);
      if (selected?.id === confirmDelete) { setSelected(null); setRuns([]); }
      setConfirmDelete(null);
      toast(t('subagent.deleted'), 'success');
      loadData();
    } catch (e) {
      const classified = classifyError(e);
      toast(classified.userMessage, 'error');
    }
  };

  const handleRun = async () => {
    if (!api?.subagent || !selected || !runInput.trim()) return;
    setRunning(true);
    setRunResult(null);
    try {
      const result = await api.subagent.run({ subagentId: selected.id, inputText: runInput.trim() });
      const run = result as SubagentRun;
      if (run.status === 'completed') {
        setRunResult(run.outputText || t('common.success'));
      } else {
        const truncated = run.errorText
          ? run.errorText.length > MAX_ERROR_LENGTH
            ? run.errorText.slice(0, MAX_ERROR_LENGTH) + '…'
            : run.errorText
          : run.id;
        setRunResult(`${run.status}: ${truncated}`);
      }
      loadRuns(selected.id);
    } catch (e) {
      const classified = classifyError(e);
      setRunResult(classified.userMessage);
    } finally { setRunning(false); }
  };

  const startEdit = (agent: Subagent) => {
    setFormName(agent.name);
    setFormRole(agent.role);
    setFormInstructions(agent.instructions);
    setFormTools(agent.tools);
    setFormProviderId(agent.providerId || '');
    setFormProviderKeyId(agent.providerKeyId || '');
    setFormModelId(agent.modelId || '');
    setFormFallbackEnabled(agent.fallbackEnabled);
    setFormMaxRuns(agent.maxRuns || 10);
    setSaveError(null);
    setEditing(true);
  };

  const resetForm = () => {
    setFormName(''); setFormRole(''); setFormInstructions(''); setFormTools('');
    setFormProviderId(''); setFormProviderKeyId(''); setFormModelId('');
    setFormFallbackEnabled(true); setFormMaxRuns(10);
  };

  const selectedFormProvider = providers.find((provider) => provider.id === formProviderId);
  const selectedProvider = selected ? providers.find((provider) => provider.id === selected.providerId) : null;
  const selectedKey = selectedProvider?.keys.find((key) => key.id === selected?.providerKeyId);

  const statusColor = (s: string) => {
    switch (s) {
      case 'completed': return '#10b981';
      case 'failed': return '#ef4444';
      case 'running': case 'queued': return '#3b82f6';
      default: return 'var(--text-disabled)';
    }
  };

  if (loading) {
    return <div className="flex-1 flex items-center justify-center"><MathCurveLoader /></div>;
  }

  if (loadError) {
    return <ErrorState message={loadError} onRetry={loadData} />;
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* Sidebar: agent list */}
      <div className="w-64 flex flex-col overflow-hidden" style={{ borderRight: '1px solid var(--border)' }}>
        <div className="px-3 py-2 flex items-center justify-between"
          style={{ borderBottom: '1px solid var(--border)' }}>
          <span className="text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>{t('subagent.title')}</span>
          <button onClick={() => { setCreating(true); setSaveError(null); }}
            style={{ color: 'var(--primary)', fontSize: '0.75rem' }}>+ {t('common.new')}</button>
        </div>
        <div className="flex-1 overflow-auto p-1 space-y-0.5">
          {agents.length === 0 && (
            <div className="p-4 text-center">
              <EmptyState title={t('subagent.empty')} description={t('subagent.emptyDesc')}
                action={{ label: t('subagent.create'), onClick: () => { setCreating(true); setSaveError(null); } }} />
            </div>
          )}
          {agents.map(agent => (
            <button key={agent.id} onClick={() => handleSelect(agent)}
              className="w-full text-left px-2 py-1.5 text-sm rounded"
              style={{
                background: selected?.id === agent.id ? 'var(--primary)' : 'transparent',
                color: selected?.id === agent.id ? '#fff' : 'var(--text)',
              }}>
              <div className="font-medium truncate">{agent.name}</div>
              {agent.role && <div className="text-xs truncate"
                style={{ color: selected?.id === agent.id ? 'rgba(255,255,255,0.7)' : 'var(--text-secondary)' }}>{agent.role}</div>}
            </button>
          ))}
        </div>
      </div>

      {/* Main content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {saveError && (
          <div className="mx-4 mt-3 rounded px-3 py-2 text-sm"
            style={{ background: 'var(--surface)', border: '1px solid var(--danger)', color: 'var(--danger)' }}>
            {saveError}
          </div>
        )}
        {selected ? (
          <div className="flex-1 overflow-auto p-4 space-y-4">
            {editing ? (
              /* Edit form */
              <div className="space-y-3 max-w-lg">
                <h2 className="text-lg font-semibold" style={{ color: 'var(--text)' }}>{t('subagent.edit')}</h2>
                <input value={formName} onChange={e => setFormName(e.target.value)} placeholder={t('subagent.name')}
                  className="w-full px-3 py-2 border rounded text-sm"
                  style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
                <input value={formRole} onChange={e => setFormRole(e.target.value)} placeholder={t('subagent.role')}
                  className="w-full px-3 py-2 border rounded text-sm"
                  style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
                <textarea value={formInstructions} onChange={e => setFormInstructions(e.target.value)} placeholder={t('subagent.instructions')}
                  className="w-full px-3 py-2 border rounded text-sm h-24 resize-none"
                  style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
                <textarea value={formTools} onChange={e => setFormTools(e.target.value)} placeholder={t('subagent.tools')}
                  className="w-full px-3 py-2 border rounded text-sm h-20 resize-none"
                  style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
                <ProviderBindingFields
                  t={t}
                  providers={providers}
                  providerId={formProviderId}
                  providerKeyId={formProviderKeyId}
                  modelId={formModelId}
                  fallbackEnabled={formFallbackEnabled}
                  maxRuns={formMaxRuns}
                  selectedProvider={selectedFormProvider}
                  onProviderChange={(value) => {
                    setFormProviderId(value);
                    setFormProviderKeyId('');
                  }}
                  onKeyChange={setFormProviderKeyId}
                  onModelChange={setFormModelId}
                  onFallbackChange={setFormFallbackEnabled}
                  onMaxRunsChange={setFormMaxRuns}
                />
                <div className="flex gap-2">
                  <button onClick={handleUpdate}
                    className="px-4 py-2 text-sm rounded"
                    style={{ background: 'var(--primary)', color: '#fff' }}>{t('common.save')}</button>
                  <button onClick={() => setEditing(false)}
                    className="px-4 py-2 text-sm"
                    style={{ color: 'var(--text-secondary)' }}>{t('common.cancel')}</button>
                </div>
              </div>
            ) : (
              /* View mode */
              <>
                <div className="flex items-center justify-between">
                  <div>
                    <h2 className="text-lg font-semibold" style={{ color: 'var(--text)' }}>{selected.name}</h2>
                    {selected.role && <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>{selected.role}</p>}
                  </div>
                  <div className="flex gap-2">
                    <button onClick={() => startEdit(selected)}
                      className="px-3 py-1 text-xs rounded"
                      style={{ background: 'var(--surface)', color: 'var(--text)' }}>{t('common.edit')}</button>
                    <button onClick={() => setConfirmDelete(selected.id)}
                      className="px-3 py-1 text-xs rounded"
                      style={{ background: 'var(--surface)', color: 'var(--danger)' }}>{t('common.delete')}</button>
                  </div>
                </div>

                {selected.instructions && (
                  <div>
                    <p className="text-xs uppercase font-semibold mb-1"
                      style={{ color: 'var(--text-secondary)' }}>{t('subagent.instructions')}</p>
                    <p className="text-sm whitespace-pre-wrap" style={{ color: 'var(--text)' }}>{selected.instructions}</p>
                  </div>
                )}
                {selected.tools && (
                  <div>
                    <p className="text-xs uppercase font-semibold mb-1"
                      style={{ color: 'var(--text-secondary)' }}>{t('subagent.tools')}</p>
                    <p className="text-sm" style={{ color: 'var(--text)' }}>{selected.tools}</p>
                  </div>
                )}
                <div className="text-xs space-y-0.5" style={{ color: 'var(--text-disabled)' }}>
                  <div>{t('subagent.provider')}: {selectedProvider?.name || t('subagent.notConfigured')}</div>
                  <div>{t('subagent.key')}: {selectedKey ? `${selectedKey.label} (${selectedKey.maskedKey})` : t('subagent.notConfigured')}</div>
                  <div>{t('subagent.model')}: {selected.modelId || t('subagent.notConfigured')}</div>
                  <div>{t('subagent.fallback')}: {selected.fallbackEnabled ? t('common.yes') : t('common.no')}</div>
                </div>
              </>
            )}

            {/* Run panel */}
            <div className="pt-4" style={{ borderTop: '1px solid var(--border)' }}>
              <h3 className="text-sm font-semibold mb-2" style={{ color: 'var(--text)' }}>{t('subagent.run')}</h3>
              <textarea value={runInput} onChange={e => setRunInput(e.target.value)}
                className="w-full px-3 py-2 border rounded text-sm h-20 resize-none"
                style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
                placeholder={t('subagent.runPlaceholder')} />
              <div className="flex gap-2 mt-2">
                <button onClick={handleRun} disabled={!runInput.trim() || running}
                  className="px-4 py-2 text-sm rounded disabled:opacity-50"
                  style={{ background: '#10b981', color: '#fff' }}>
                  {running ? t('common.running') : t('subagent.run')}
                </button>
              </div>
              {runResult && (
                <p className="text-sm mt-2" style={{
                  color: runResult.startsWith('failed') || runResult.startsWith('error')
                    ? 'var(--danger)' : 'var(--text-secondary)',
                }}>
                  {runResult}
                </p>
              )}
            </div>

            {/* Runs history */}
            <div className="pt-4" style={{ borderTop: '1px solid var(--border)' }}>
              <h3 className="text-sm font-semibold mb-2" style={{ color: 'var(--text)' }}>{t('subagent.runHistory')}</h3>
              {runs.length === 0 ? (
                <p className="text-sm" style={{ color: 'var(--text-disabled)' }}>{t('subagent.noRuns')}</p>
              ) : (
                <div className="space-y-1">
                  {runs.slice(0, 10).map(run => (
                    <div key={run.id} className="text-xs border rounded p-2"
                      style={{ borderColor: 'var(--border)' }}>
                      <div className="flex justify-between">
                        <span className="font-medium" style={{ color: statusColor(run.status) }}>{run.status}</span>
                        <span style={{ color: 'var(--text-disabled)' }}>{new Date(run.startedAt).toLocaleString()}</span>
                      </div>
                      <div className="truncate mt-0.5" style={{ color: 'var(--text-secondary)' }}>{run.inputText}</div>
                      {run.outputText && <div style={{ color: '#10b981' }} className="truncate">{run.outputText}</div>}
                      {run.errorText && (
                        <div style={{ color: 'var(--danger)' }} className="truncate">
                          {run.errorText.length > MAX_ERROR_LENGTH
                            ? run.errorText.slice(0, MAX_ERROR_LENGTH) + '…'
                            : run.errorText}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="flex-1 flex items-center justify-center" style={{ color: 'var(--text-disabled)' }}>
            <EmptyState title={t('subagent.selectAgent')} description={t('subagent.selectAgentDesc')} />
          </div>
        )}
      </div>

      {/* Create dialog */}
      {creating && (
        <div className="fixed inset-0 z-50 flex items-center justify-center" style={{ backgroundColor: 'rgba(0,0,0,0.4)' }}>
          <div className="rounded-lg p-6 w-96 shadow-xl"
            style={{ background: 'var(--surface)', border: '1px solid var(--border)' }}>
            <h2 className="text-lg font-semibold mb-4" style={{ color: 'var(--text)' }}>{t('subagent.create')}</h2>
            <div className="space-y-3">
              <input value={formName} onChange={e => setFormName(e.target.value)} placeholder={t('subagent.name')}
                className="w-full px-3 py-2 border rounded" autoFocus
                style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
              <input value={formRole} onChange={e => setFormRole(e.target.value)} placeholder={t('subagent.role')}
                className="w-full px-3 py-2 border rounded"
                style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
              <textarea value={formInstructions} onChange={e => setFormInstructions(e.target.value)} placeholder={t('subagent.instructions')}
                className="w-full px-3 py-2 border rounded h-24 resize-none"
                style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
              <textarea value={formTools} onChange={e => setFormTools(e.target.value)} placeholder={t('subagent.tools')}
                className="w-full px-3 py-2 border rounded h-20 resize-none"
                style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
              <ProviderBindingFields
                t={t}
                providers={providers}
                providerId={formProviderId}
                providerKeyId={formProviderKeyId}
                modelId={formModelId}
                fallbackEnabled={formFallbackEnabled}
                maxRuns={formMaxRuns}
                selectedProvider={selectedFormProvider}
                onProviderChange={(value) => {
                  setFormProviderId(value);
                  setFormProviderKeyId('');
                }}
                onKeyChange={setFormProviderKeyId}
                onModelChange={setFormModelId}
                onFallbackChange={setFormFallbackEnabled}
                onMaxRunsChange={setFormMaxRuns}
              />
              {saveError && (
                <p className="text-sm" style={{ color: 'var(--danger)' }}>{saveError}</p>
              )}
            </div>
            <div className="flex justify-end gap-2 mt-4">
              <button onClick={() => setCreating(false)}
                className="px-4 py-2 text-sm"
                style={{ color: 'var(--text-secondary)' }}>{t('common.cancel')}</button>
              <button onClick={handleCreate} disabled={!formName.trim()}
                className="px-4 py-2 text-sm rounded disabled:opacity-50"
                style={{ background: 'var(--primary)', color: '#fff' }}>{t('common.create')}</button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm delete */}
      <ConfirmDialog
        open={confirmDelete !== null}
        title={t('subagent.deleteConfirm')}
        message={t('subagent.deleteMessage')}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        danger
        onConfirm={handleDelete}
        onCancel={() => setConfirmDelete(null)}
      />
    </div>
  );
}

function ProviderBindingFields({
  t,
  providers,
  providerId,
  providerKeyId,
  modelId,
  fallbackEnabled,
  maxRuns,
  selectedProvider,
  onProviderChange,
  onKeyChange,
  onModelChange,
  onFallbackChange,
  onMaxRunsChange,
}: {
  t: (key: string) => string;
  providers: UserProvider[];
  providerId: string;
  providerKeyId: string;
  modelId: string;
  fallbackEnabled: boolean;
  maxRuns: number;
  selectedProvider?: UserProvider;
  onProviderChange: (value: string) => void;
  onKeyChange: (value: string) => void;
  onModelChange: (value: string) => void;
  onFallbackChange: (value: boolean) => void;
  onMaxRunsChange: (value: number) => void;
}) {
  return (
    <div className="space-y-2 rounded p-3" style={{ border: '1px solid var(--border)' }}>
      <select value={providerId} onChange={(e) => onProviderChange(e.target.value)}
        className="w-full px-3 py-2 border rounded text-sm"
        style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}>
        <option value="">{t('subagent.selectProvider')}</option>
        {providers.map((provider) => (
          <option key={provider.id} value={provider.id}>{provider.name}</option>
        ))}
      </select>
      <select value={providerKeyId} onChange={(e) => onKeyChange(e.target.value)} disabled={!selectedProvider}
        className="w-full px-3 py-2 border rounded text-sm disabled:opacity-60"
        style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}>
        <option value="">{t('subagent.selectKey')}</option>
        {selectedProvider?.keys.map((key) => (
          <option key={key.id} value={key.id}>{key.label} ({key.maskedKey})</option>
        ))}
      </select>
      <input value={modelId} onChange={(e) => onModelChange(e.target.value)}
        placeholder={t('subagent.modelPlaceholder')}
        className="w-full px-3 py-2 border rounded text-sm"
        style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
      <div className="grid grid-cols-2 gap-2">
        <label className="flex items-center gap-2 text-xs"
          style={{ color: 'var(--text-secondary)' }}>
          <input type="checkbox" checked={fallbackEnabled}
            onChange={(e) => onFallbackChange(e.target.checked)} />
          {t('subagent.fallback')}
        </label>
        <input type="number" min={1} max={50} value={maxRuns}
          onChange={(e) => onMaxRunsChange(Math.max(1, Number(e.target.value) || 1))}
          className="px-3 py-2 border rounded text-sm"
          style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
          aria-label={t('subagent.maxRuns')} />
      </div>
    </div>
  );
}
