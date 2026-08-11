'use client';

import { useMemo, useState } from 'react';
import {
  AlertCircle,
  Ban,
  Check,
  CircleAlert,
  Edit3,
  KeyRound,
  Loader,
  Plus,
  RefreshCw,
  Search,
  Server,
  Star,
  Trash2,
  Wifi,
  X,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { ProviderKeyStatus, ProviderKeySummary, ProviderSummary, TestKeyResult } from '@/types/provider';
import { classifyError } from '@/lib/error-classifier';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';

const STATUS_ICON: Record<ProviderKeyStatus, React.ReactNode> = {
  valid: <Check size={12} />,
  invalid: <X size={12} />,
  rate_limited: <AlertCircle size={12} />,
  unavailable: <Ban size={12} />,
  untested: <CircleAlert size={12} />,
};

function keyStatusLabel(locale: Locale, status: ProviderKeyStatus) {
  const keys: Record<ProviderKeyStatus, string> = {
    valid: 'providerDetail.statusReady',
    invalid: 'providerDetail.statusInvalid',
    rate_limited: 'providerDetail.statusLimited',
    unavailable: 'providerDetail.statusUnavailable',
    untested: 'providerDetail.statusUntested',
  };
  return t(locale, keys[status]);
}

export default function ProviderDetail({ locale, providers, loading, showAddProvider, onSaveDefaults, onAddKey, onTestKey, onSetPrimaryKey, onDeleteKey, onDeleteProvider }: {
  locale: Locale;
  providers: ProviderSummary[];
  loading: boolean;
  showAddProvider: () => void;
  onSaveDefaults: (pid: string, model: string | null) => Promise<void>;
  onAddKey: (pid: string, label: string, key: string) => Promise<void>;
  onTestKey: (pid: string, keyId: string, model?: string) => Promise<TestKeyResult>;
  onSetPrimaryKey: (pid: string, keyId: string) => Promise<void>;
  onDeleteKey: (pid: string, keyId: string) => Promise<void>;
  onDeleteProvider: (pid: string) => void;
}) {
  const { toast } = useToast();
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(providers[0]?.id ?? null);
  
  // ── 编辑模式控制 ──
  const [isEditingModel, setIsEditingModel] = useState(false);

  const [models, setModels] = useState<Record<string, string>>({});
  const [savingModel, setSavingModel] = useState<string | null>(null);
  const [modelErrors, setModelErrors] = useState<Record<string, string>>({});
  const [saveSuccessMsg, setSaveSuccessMsg] = useState<string | null>(null);

  const [newLabel, setNewLabel] = useState('');
  const [newKey, setNewKey] = useState('');
  const [addingKey, setAddingKey] = useState<string | null>(null);
  const [addError, setAddError] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, { status: ProviderKeyStatus; testedAt: string; error: string | null }>>({});
  const [deleteTarget, setDeleteTarget] = useState<{ providerId: string; keyId: string } | null>(null);

  const [discovered, setDiscovered] = useState<Record<string, Array<{ id: string; displayName?: string }>>>({});
  const [discovering, setDiscovering] = useState(false);
  const [testing, setTesting] = useState(false);
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [detailTestResult, setDetailTestResult] = useState<{ success: boolean; error?: string } | null>(null);

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return needle
      ? providers.filter((provider) => `${provider.displayName} ${provider.baseUrl}`.toLowerCase().includes(needle))
      : providers;
  }, [providers, query]);
  const selected = providers.find((provider) => provider.id === selectedId) ?? filtered[0] ?? null;

  const handleSelectProvider = (pid: string) => {
    setSelectedId(pid);
    setIsEditingModel(false);
    setAddError(null);
    setSaveSuccessMsg(null);
    setDetailTestResult(null);
  };

  if (loading) {
    return <div className="settings-state"><Loader size={22} className="animate-spin" /><span>{t(locale, 'settings.providersLoading')}</span></div>;
  }

  if (providers.length === 0) {
    return (
      <div className="settings-state settings-state-empty">
        <span className="settings-state-icon"><Server size={22} /></span>
        <strong>{t(locale, 'settings.noProviders')}</strong>
        <span>{t(locale, 'providerDetail.addProviderHint')}</span>
        <button className="btn btn-primary" onClick={showAddProvider}><Plus size={15} />{t(locale, 'settings.addProvider')}</button>
      </div>
    );
  }

  const startEdit = () => {
    if (!selected) return;
    setModels((current) => ({
      ...current,
      [selected.id]: current[selected.id] ?? selected.defaultModel ?? '',
    }));
    setIsEditingModel(true);
    setSaveSuccessMsg(null);
    setModelErrors((current) => ({ ...current, [selected.id]: '' }));
  };

  const cancelEdit = () => {
    if (!selected) return;
    setModels((current) => ({
      ...current,
      [selected.id]: selected.defaultModel ?? '',
    }));
    setIsEditingModel(false);
    setSaveSuccessMsg(null);
    setModelErrors((current) => ({ ...current, [selected.id]: '' }));
    setDetailTestResult(null);
  };

  const saveModel = async () => {
    if (!selected) return;
    const value = (models[selected.id] ?? selected.defaultModel ?? '').trim();
    setSavingModel(selected.id);
    setModelErrors((current) => ({ ...current, [selected.id]: '' }));
    setSaveSuccessMsg(null);
    try {
      await onSaveDefaults(selected.id, value || null);
      const msg = t(locale, 'providerDetail.saved');
      setSaveSuccessMsg(msg);
      toast(msg, 'success');
      setIsEditingModel(false);
    } catch (error) {
      const errMsg = classifyError(error, { locale }).userMessage;
      setModelErrors((current) => ({ ...current, [selected.id]: errMsg }));
      toast(errMsg, 'error');
    } finally {
      setSavingModel(null);
    }
  };

  const addKey = async () => {
    if (!selected || !newKey.trim()) return;
    setAddingKey(selected.id);
    setAddError(null);
    const keyName = newLabel.trim() || `Key ${selected.keys.length + 1}`;
    try {
      await onAddKey(selected.id, keyName, newKey);
      setNewLabel('');
      setNewKey('');
      toast(t(locale, 'providerDetail.keyAdded', { name: keyName }), 'success');
    } catch (error) {
      const errMsg = classifyError(error, { locale }).userMessage;
      setAddError(errMsg);
      toast(errMsg, 'error');
    } finally {
      setAddingKey(null);
    }
  };

  const testKey = async (key: ProviderKeySummary) => {
    if (!selected || testingId) return;
    setTestingId(key.id);
    try {
      const result = await onTestKey(selected.id, key.id);
      const isOk = result.status === 'valid';
      setTestResults((current) => ({ ...current, [key.id]: { status: result.status, testedAt: result.testedAt, error: result.userMessage } }));
      toast(
        isOk
          ? t(locale, 'providerDetail.keyTestSucceeded', { name: key.label })
          : t(locale, 'providerDetail.keyTestFailed', { name: key.label, detail: result.userMessage || t(locale, 'providerDetail.cannotConnect') }),
        isOk ? 'success' : 'error'
      );
    } catch (error) {
      const errMsg = classifyError(error, { locale }).userMessage;
      setTestResults((current) => ({ ...current, [key.id]: { status: 'unavailable', testedAt: new Date().toISOString(), error: errMsg } }));
      toast(t(locale, 'providerDetail.keyTestFailedSimple', { name: key.label }), 'error');
    } finally {
      setTestingId(null);
    }
  };

  const handleSetPrimaryKey = async (keyId: string) => {
    if (!selected) return;
    try {
      await onSetPrimaryKey(selected.id, keyId);
      toast(t(locale, 'providerDetail.primaryKeySet'), 'success');
    } catch (error) {
      toast(classifyError(error, { locale }).userMessage, 'error');
    }
  };

  const handleConfirmDeleteKey = async () => {
    if (!deleteTarget) return;
    try {
      await onDeleteKey(deleteTarget.providerId, deleteTarget.keyId);
      toast(t(locale, 'providerDetail.keyDeleted'), 'success');
    } catch (error) {
      toast(classifyError(error, { locale }).userMessage, 'error');
    } finally {
      setDeleteTarget(null);
    }
  };

  const discoverModels = async () => {
    if (!selected || !selected.primaryKeyId || discovering) return;
    setDiscovering(true);
    try {
      const providerApi = window.nativesAPI?.provider;
      if (!providerApi?.discoverModelsSaved) throw new Error(t(locale, 'providerDetail.discoveryUnsupported'));
      const result = await providerApi.discoverModelsSaved({
        providerId: selected.id,
        keyId: selected.primaryKeyId,
      });
      setDiscovered((current) => ({ ...current, [selected.id]: result }));
      if (result.length > 0) {
        const currentValue = (models[selected.id] ?? selected.defaultModel ?? '').trim();
        const firstModel = result[0];
        if (!currentValue && firstModel) {
          setModels((current) => ({ ...current, [selected.id]: firstModel.id }));
        }
        setDropdownOpen(true);
        toast(t(locale, 'providerDetail.modelsDiscovered', { count: result.length }), 'success');
      } else {
        toast(t(locale, 'providerDetail.noModelsDiscovered'), 'warning');
      }
    } catch (error) {
      const errMsg = classifyError(error, { locale }).userMessage;
      setModelErrors((current) => ({ ...current, [selected.id]: errMsg }));
      toast(errMsg, 'error');
    } finally {
      setDiscovering(false);
    }
  };

  const testModelConnection = async () => {
    if (!selected || !selected.primaryKeyId || testing) return;
    const value = (models[selected.id] ?? selected.defaultModel ?? '').trim();
    if (!value) {
      const msg = t(locale, 'providerDetail.enterOrSelectModel');
      setDetailTestResult({ success: false, error: msg });
      toast(msg, 'warning');
      return;
    }
    setTesting(true);
    setDetailTestResult(null);
    try {
      const result = await onTestKey(selected.id, selected.primaryKeyId, value);
      const isOk = result.status === 'valid';
      const resMsg = result.userMessage ?? undefined;
      setDetailTestResult({ success: isOk, error: resMsg });
      toast(
        isOk
          ? t(locale, 'providerDetail.modelTestSucceeded', { name: value })
          : t(locale, 'providerDetail.modelTestFailed', { name: value }),
        isOk ? 'success' : 'error'
      );
    } catch (error) {
      const errMsg = classifyError(error, { locale }).userMessage;
      setDetailTestResult({ success: false, error: errMsg });
      toast(errMsg, 'error');
    } finally {
      setTesting(false);
    }
  };

  const currentModelDisplay = models[selected?.id ?? ''] ?? selected?.defaultModel ?? '';

  return (
    <section className="provider-workspace">
      <aside className="provider-list-panel">
        <div className="provider-search">
          <Search size={15} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t(locale, 'settings.searchProvider')} />
        </div>
        <div className="provider-list">
          {filtered.map((provider) => {
            const active = selected?.id === provider.id;
            const ready = provider.keys.some((key) => key.isPrimary);
            return (
              <button
                key={provider.id}
                type="button"
                className={`provider-list-item${active ? ' active' : ''}`}
                onClick={() => handleSelectProvider(provider.id)}
              >
                <span className="provider-avatar">{provider.displayName.slice(0, 1).toUpperCase()}</span>
                <span className="provider-list-copy">
                  <strong>{provider.displayName}</strong>
                  <span>{provider.keys.length} Key{provider.keys.length === 1 ? '' : 's'}</span>
                </span>
                <span className={`provider-health${ready ? ' ready' : ''}`} aria-label={ready ? 'ready' : 'not configured'} />
              </button>
            );
          })}
          {filtered.length === 0 && <div className="provider-list-empty">{t(locale, 'settings.noProviderMatch')}</div>}
        </div>
      </aside>

      {selected && (
        <div className="provider-detail-panel">
          <div className="provider-detail-header">
            <div className="provider-title-group">
              <span className="provider-avatar provider-avatar-large">{selected.displayName.slice(0, 1).toUpperCase()}</span>
              <div>
                <div className="provider-title-line">
                  <h3>{selected.displayName}</h3>
                  {selected.primaryKeyId && <span className="settings-badge">{t(locale, 'providerDetail.connected')}</span>}
                </div>
                <code>{selected.baseUrl}</code>
              </div>
            </div>
            <button type="button" className="settings-icon-button danger" onClick={() => onDeleteProvider(selected.id)} title={t(locale, 'settings.deleteProvider')} aria-label={t(locale, 'settings.deleteProvider')}><Trash2 size={16} /></button>
          </div>

          <div className="provider-detail-body">
            {/* ── 基础配置 ── */}
            <section className="settings-section-card">
              <div className="settings-section-heading settings-section-heading-row">
                <div>
                  <h4>{t(locale, 'providerDetail.configuration')}</h4>
                  <p>{t(locale, 'providerDetail.configDesc')}</p>
                </div>
                {!isEditingModel ? (
                  <button
                    type="button"
                    className="btn"
                    onClick={startEdit}
                    title={t(locale, 'providerDetail.editConfiguration')}
                  >
                    <Edit3 size={14} />
                    {t(locale, 'providerDetail.edit')}
                  </button>
                ) : (
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      className="btn"
                      onClick={cancelEdit}
                      disabled={savingModel === selected.id}
                      title={t(locale, 'providerDetail.cancelEdit')}
                    >
                      <X size={14} />
                      {t(locale, 'common.cancel')}
                    </button>
                    <button
                      type="button"
                      className="btn btn-primary"
                      onClick={saveModel}
                      disabled={savingModel === selected.id}
                      title={t(locale, 'providerDetail.saveDefaultModel')}
                    >
                      {savingModel === selected.id ? <Loader size={14} className="animate-spin" /> : <Check size={14} />}
                      {t(locale, 'providerDetail.saveConfig')}
                    </button>
                  </div>
                )}
              </div>

              {!isEditingModel ? (
                /* ── 非编辑模式：只读预览态 ── */
                <div className="settings-field-grid">
                  <div className="settings-readonly-field">
                    <span>{t(locale, 'settings.providerName')}</span>
                    <strong>{selected.displayName}</strong>
                  </div>
                  <div className="settings-readonly-field">
                    <span>{t(locale, 'settings.baseUrl')}</span>
                    <code>{selected.baseUrl}</code>
                  </div>
                  <div className="settings-readonly-field">
                    <span>{t(locale, 'assistant.defaultModel')}</span>
                    <code className="text-xs font-semibold">
                      {selected.defaultModel || t(locale, 'providerDetail.noDefaultModel')}
                    </code>
                  </div>
                </div>
              ) : (
                /* ── 编辑模式：激活编辑表单 ── */
                <>
                  <div className="settings-field-grid">
                    <div className="settings-readonly-field">
                      <span>{t(locale, 'settings.providerName')}</span>
                      <strong>{selected.displayName}</strong>
                    </div>
                    <div className="settings-readonly-field">
                      <span>{t(locale, 'settings.baseUrl')}</span>
                      <code>{selected.baseUrl}</code>
                    </div>
                  </div>

                  <div style={{ marginTop: '12px' }}>
                    <label className="settings-control-label" htmlFor={`model-${selected.id}`}>
                      {t(locale, 'assistant.defaultModel')}
                    </label>
                    <div className="add-provider-connection-row" style={{ marginTop: '4px' }}>
                      <div style={{ position: 'relative', flex: 1, minWidth: 0 }}>
                        <input
                          id={`model-${selected.id}`}
                          className="settings-input"
                          value={currentModelDisplay}
                          onChange={(event) => {
                            setModels((current) => ({ ...current, [selected.id]: event.target.value }));
                            setDetailTestResult(null);
                          }}
                          onFocus={() => {
                            if ((discovered[selected.id]?.length ?? 0) > 0) setDropdownOpen(true);
                          }}
                          placeholder="gpt-4o"
                          style={{ paddingRight: '32px' }}
                          autoFocus
                        />
                        {(discovered[selected.id]?.length ?? 0) > 0 && (
                          <button
                            type="button"
                            onClick={() => setDropdownOpen(!dropdownOpen)}
                            style={{
                              position: 'absolute',
                              right: 0,
                              top: 0,
                              bottom: 0,
                              width: '32px',
                              background: 'none',
                              border: 'none',
                              cursor: 'pointer',
                              display: 'flex',
                              alignItems: 'center',
                              justifyContent: 'center',
                              color: 'var(--text-secondary)',
                            }}
                          >
                            <svg width="10" height="6" viewBox="0 0 10 6" fill="none" xmlns="http://www.w3.org/2000/svg">
                              <path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                            </svg>
                          </button>
                        )}

                        {dropdownOpen && (discovered[selected.id]?.length ?? 0) > 0 && (
                          <>
                            <div
                              role="presentation"
                              style={{ position: 'fixed', inset: 0, zIndex: 998 }}
                              onClick={() => setDropdownOpen(false)}
                            />
                            <div
                              style={{
                                position: 'absolute',
                                top: '100%',
                                left: 0,
                                right: 0,
                                marginTop: '4px',
                                maxHeight: '200px',
                                overflowY: 'auto',
                                background: 'var(--surface)',
                                border: '1px solid var(--border)',
                                borderRadius: 'var(--radius-sm)',
                                boxShadow: 'var(--shadow-popup)',
                                zIndex: 999,
                              }}
                            >
                              {discovered[selected.id]?.map((model) => (
                                <button
                                  key={model.id}
                                  type="button"
                                  onClick={() => {
                                    setModels((current) => ({ ...current, [selected.id]: model.id }));
                                    setDetailTestResult(null);
                                    setDropdownOpen(false);
                                  }}
                                  style={{
                                    width: '100%',
                                    padding: '8px 12px',
                                    textAlign: 'left',
                                    background: 'transparent',
                                    border: 'none',
                                    color: 'var(--text)',
                                    fontSize: '13px',
                                    cursor: 'pointer',
                                    outline: 'none',
                                  }}
                                  onMouseEnter={(e) => {
                                    e.currentTarget.style.background = 'var(--surface-hover)';
                                  }}
                                  onMouseLeave={(e) => {
                                    e.currentTarget.style.background = 'transparent';
                                  }}
                                >
                                  {model.displayName ?? model.id}
                                </button>
                              ))}
                            </div>
                          </>
                        )}
                      </div>
                      <button
                        type="button"
                        className="btn"
                        onClick={discoverModels}
                        disabled={!selected.primaryKeyId || discovering}
                        title={t(locale, 'providerDetail.fetchModelsTitle')}
                      >
                        {discovering ? <Loader size={14} className="animate-spin" /> : <RefreshCw size={14} />}
                        {t(locale, 'providerDetail.fetchModels')}
                      </button>
                      <button
                        type="button"
                        className="btn"
                        onClick={testModelConnection}
                        disabled={!selected.primaryKeyId || testing}
                        title={t(locale, 'providerDetail.testConnectionTitle')}
                      >
                        {testing ? <Loader size={14} className="animate-spin" /> : <Wifi size={14} />}
                        {t(locale, 'providerDetail.test')}
                      </button>
                    </div>
                  </div>
                </>
              )}

              {saveSuccessMsg && (
                <div className="add-provider-message success" style={{ marginTop: '8px' }}>
                  <Check size={14} />
                  {saveSuccessMsg}
                </div>
              )}

              {detailTestResult && (
                <div
                  className={`add-provider-message ${detailTestResult.success ? 'success' : 'error'}`}
                  style={{ marginTop: '8px' }}
                >
                  {detailTestResult.success ? (
                    <>
                      <Check size={14} />
                      {t(locale, 'providerDetail.connectionTestSucceeded')}
                    </>
                  ) : (
                    detailTestResult.error || t(locale, 'providerDetail.connectionTestFailed')
                  )}
                </div>
              )}
              {modelErrors[selected.id] && <p className="settings-error">{modelErrors[selected.id]}</p>}
            </section>

            {/* ── API Keys ── */}
            <section className="settings-section-card">
              <div className="settings-section-heading settings-section-heading-row">
                <div><h4>API Keys</h4><p>{t(locale, 'providerDetail.keysDesc')}</p></div>
                <span className="settings-count">{selected.keys.filter((key) => key.isActive).length} / {selected.keys.length}</span>
              </div>

              {selected.keys.length === 0 ? (
                <div className="provider-key-empty">
                  <KeyRound size={18} />
                  {t(locale, 'providerDetail.noKeys')}
                </div>
              ) : (
                <div className="provider-key-list">
                  {selected.keys.map((key) => {
                    const result = testResults[key.id];
                    const status = result?.status ?? key.status;
                    const testedAt = result?.testedAt ?? key.lastTestedAt;
                    const error = result?.error ?? key.lastError;
                    const testing = testingId === key.id;
                    return (
                      <div className="provider-key-row" key={key.id}>
                        <span className="provider-key-icon"><KeyRound size={15} /></span>
                        <div className="provider-key-main">
                          <div className="provider-key-name">{key.label}{key.isPrimary && <span className="settings-badge neutral"><Star size={10} />{t(locale, 'assistant.primaryKey')}</span>}</div>
                          <code>{key.maskedKey}</code>
                          {error && <span className="settings-error inline"><AlertCircle size={11} />{error}</span>}
                        </div>
                        <div className="provider-key-meta">
                          <span className={`key-status ${status}`}>{STATUS_ICON[status]}{keyStatusLabel(locale, status)}</span>
                          <span>{testedAt ? new Date(testedAt).toLocaleDateString(locale === 'zh' ? 'zh-CN' : 'en-US', { month: 'short', day: 'numeric' }) : t(locale, 'providerDetail.notTested')}</span>
                        </div>
                        <div className="provider-key-actions">
                          {/* 测试连接在任何模式下均可用 */}
                          <button type="button" onClick={() => testKey(key)} disabled={testing} className="settings-icon-button" title={t(locale, 'providerDetail.testConnection')}>{testing ? <Loader size={14} className="animate-spin" /> : <Wifi size={14} />}</button>
                          
                          {/* 编辑模式下开放 设为主 Key 和 删除 选项 */}
                          {isEditingModel && !key.isPrimary && (
                            <button type="button" onClick={() => handleSetPrimaryKey(key.id)} disabled={key.status !== 'valid'} className="settings-icon-button" title={key.status !== 'valid' ? t(locale, 'providerDetail.testFirst') : t(locale, 'assistant.setPrimaryKey')}><Star size={14} /></button>
                          )}
                          {isEditingModel && !key.isPrimary && (
                            <button type="button" onClick={() => setDeleteTarget({ providerId: selected.id, keyId: key.id })} className="settings-icon-button danger" title={t(locale, 'providerDetail.deleteKey')}><Trash2 size={14} /></button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}

              {/* 仅在点击“编辑配置”进入编辑模式后，才渲染“添加新 Key”的输入框和“添加”按钮 */}
              {isEditingModel && (
                <div className="provider-add-key" style={{ marginTop: '16px' }}>
                  <div className="provider-add-key-title"><Plus size={14} />{t(locale, 'providerDetail.addKey')}</div>
                  <div className="provider-add-key-fields">
                    <input className="settings-input" value={newLabel} onChange={(event) => setNewLabel(event.target.value)} placeholder={t(locale, 'providerDetail.labelPlaceholder')} />
                    <input className="settings-input" type="password" value={newKey} onChange={(event) => setNewKey(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && addKey()} placeholder={t(locale, 'providerDetail.apiKeyPlaceholder')} />
                    <button type="button" className="btn btn-primary" onClick={addKey} disabled={!newKey.trim() || addingKey === selected.id}>{addingKey === selected.id ? <Loader size={14} className="animate-spin" /> : <Plus size={14} />}{t(locale, 'providerDetail.add')}</button>
                  </div>
                  {addError && <p className="settings-error">{addError}</p>}
                </div>
              )}
            </section>
          </div>
        </div>
      )}

      <ConfirmDialog open={deleteTarget !== null} title={t(locale, 'providerDetail.deleteKey')} message={t(locale, 'providerDetail.deleteKeyMessage')} confirmLabel={t(locale, 'common.delete')} cancelLabel={t(locale, 'common.cancel')} danger onConfirm={handleConfirmDeleteKey} onCancel={() => setDeleteTarget(null)} />
    </section>
  );
}
