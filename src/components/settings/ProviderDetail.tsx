'use client';

import { useMemo, useState } from 'react';
import {
  AlertCircle,
  Ban,
  Check,
  CircleAlert,
  KeyRound,
  Loader,
  Plus,
  Search,
  Server,
  Star,
  Trash2,
  Wifi,
  X,
  RefreshCw,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { ProviderKeyStatus, ProviderKeySummary, ProviderSummary, TestKeyResult } from '@/types/provider';
import { classifyError } from '@/lib/error-classifier';
import ConfirmDialog from '@/components/ui/ConfirmDialog';

const STATUS_ICON: Record<ProviderKeyStatus, React.ReactNode> = {
  valid: <Check size={12} />,
  invalid: <X size={12} />,
  rate_limited: <AlertCircle size={12} />,
  unavailable: <Ban size={12} />,
  untested: <CircleAlert size={12} />,
};

function keyStatusLabel(locale: Locale, status: ProviderKeyStatus) {
  const zh = locale === 'zh';
  return {
    valid: zh ? '可用' : 'Ready',
    invalid: zh ? '无效' : 'Invalid',
    rate_limited: zh ? '受限' : 'Limited',
    unavailable: zh ? '不可用' : 'Unavailable',
    untested: zh ? '未测试' : 'Untested',
  }[status];
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
  const zh = locale === 'zh';
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(providers[0]?.id ?? null);
  const [models, setModels] = useState<Record<string, string>>({});
  const [savingModel, setSavingModel] = useState<string | null>(null);
  const [modelErrors, setModelErrors] = useState<Record<string, string>>({});
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

  if (loading) {
    return <div className="settings-state"><Loader size={22} className="animate-spin" /><span>{t(locale, 'settings.providersLoading')}</span></div>;
  }

  if (providers.length === 0) {
    return (
      <div className="settings-state settings-state-empty">
        <span className="settings-state-icon"><Server size={22} /></span>
        <strong>{t(locale, 'settings.noProviders')}</strong>
        <span>{zh ? '添加一个供应商后，即可配置模型和 API Key。' : 'Add a provider to configure models and API keys.'}</span>
        <button className="btn btn-primary" onClick={showAddProvider}><Plus size={15} />{t(locale, 'settings.addProvider')}</button>
      </div>
    );
  }

  const saveModel = async () => {
    if (!selected) return;
    const value = (models[selected.id] ?? selected.defaultModel ?? '').trim();
    setSavingModel(selected.id);
    setModelErrors((current) => ({ ...current, [selected.id]: '' }));
    try {
      await onSaveDefaults(selected.id, value || null);
    } catch (error) {
      setModelErrors((current) => ({ ...current, [selected.id]: classifyError(error, { locale }).userMessage }));
    } finally {
      setSavingModel(null);
    }
  };

  const addKey = async () => {
    if (!selected || !newKey.trim()) return;
    setAddingKey(selected.id);
    setAddError(null);
    try {
      await onAddKey(selected.id, newLabel.trim() || `Key ${selected.keys.length + 1}`, newKey);
      setNewLabel('');
      setNewKey('');
    } catch (error) {
      setAddError(classifyError(error, { locale }).userMessage);
    } finally {
      setAddingKey(null);
    }
  };

  const testKey = async (key: ProviderKeySummary) => {
    if (!selected || testingId) return;
    setTestingId(key.id);
    try {
      const result = await onTestKey(selected.id, key.id);
      setTestResults((current) => ({ ...current, [key.id]: { status: result.status, testedAt: result.testedAt, error: result.userMessage } }));
    } catch (error) {
      setTestResults((current) => ({ ...current, [key.id]: { status: 'unavailable', testedAt: new Date().toISOString(), error: classifyError(error, { locale }).userMessage } }));
    } finally {
      setTestingId(null);
    }
  };

  const discoverModels = async () => {
    if (!selected || !selected.primaryKeyId || discovering) return;
    setDiscovering(true);
    try {
      const providerApi = window.nativesAPI?.provider;
      if (!providerApi?.discoverModelsSaved) throw new Error(zh ? '不支持的模型获取' : 'Model discovery not available');
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
      }
    } catch (error) {
      setModelErrors((current) => ({ ...current, [selected.id]: classifyError(error, { locale }).userMessage }));
    } finally {
      setDiscovering(false);
    }
  };

  const testModelConnection = async () => {
    if (!selected || !selected.primaryKeyId || testing) return;
    const value = (models[selected.id] ?? selected.defaultModel ?? '').trim();
    if (!value) {
      setDetailTestResult({ success: false, error: zh ? '请先输入或选择一个模型' : 'Please enter or select a model first' });
      return;
    }
    setTesting(true);
    setDetailTestResult(null);
    try {
      const result = await onTestKey(selected.id, selected.primaryKeyId, value);
      setDetailTestResult({ success: result.status === 'valid', error: result.userMessage ?? undefined });
    } catch (error) {
      setDetailTestResult({ success: false, error: classifyError(error, { locale }).userMessage });
    } finally {
      setTesting(false);
    }
  };

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
              <button key={provider.id} type="button" className={`provider-list-item${active ? ' active' : ''}`} onClick={() => { setSelectedId(provider.id); setAddError(null); }}>
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
                  {selected.primaryKeyId && <span className="settings-badge">{zh ? '已连接' : 'Connected'}</span>}
                </div>
                <code>{selected.baseUrl}</code>
              </div>
            </div>
            <button type="button" className="settings-icon-button danger" onClick={() => onDeleteProvider(selected.id)} title={t(locale, 'settings.deleteProvider')} aria-label={t(locale, 'settings.deleteProvider')}><Trash2 size={16} /></button>
          </div>

          <div className="provider-detail-body">
            <section className="settings-section-card">
              <div className="settings-section-heading">
                <div><h4>{zh ? '基础配置' : 'Configuration'}</h4><p>{zh ? '请求地址由供应商预设提供，只需指定默认模型。' : 'The provider preset supplies the endpoint; choose the default model.'}</p></div>
              </div>
              <div className="settings-field-grid">
                <div className="settings-readonly-field"><span>{t(locale, 'settings.providerName')}</span><strong>{selected.displayName}</strong></div>
                <div className="settings-readonly-field"><span>{t(locale, 'settings.baseUrl')}</span><code>{selected.baseUrl}</code></div>
              </div>
              <label className="settings-control-label" htmlFor={`model-${selected.id}`}>{t(locale, 'assistant.defaultModel')}</label>
              <div className="add-provider-connection-row" style={{ marginTop: '4px' }}>
                <div style={{ position: 'relative', flex: 1, minWidth: 0 }}>
                  <input
                    id={`model-${selected.id}`}
                    className="settings-input"
                    value={models[selected.id] ?? selected.defaultModel ?? ''}
                    onChange={(event) => {
                      setModels((current) => ({ ...current, [selected.id]: event.target.value }));
                      setDetailTestResult(null);
                    }}
                    onFocus={() => {
                      if ((discovered[selected.id]?.length ?? 0) > 0) setDropdownOpen(true);
                    }}
                    placeholder="gpt-4o"
                    style={{ paddingRight: '32px' }}
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
                  title={zh ? '获取可用模型列表' : 'Fetch available models'}
                >
                  {discovering ? <Loader size={14} className="animate-spin" /> : <RefreshCw size={14} />}
                  {zh ? '获取模型' : 'Fetch'}
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={testModelConnection}
                  disabled={!selected.primaryKeyId || testing}
                  title={zh ? '测试模型连接' : 'Test model connection'}
                >
                  {testing ? <Loader size={14} className="animate-spin" /> : <Wifi size={14} />}
                  {zh ? '测试' : 'Test'}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={saveModel}
                  disabled={savingModel === selected.id}
                  title={zh ? '保存默认模型' : 'Save default model'}
                >
                  {savingModel === selected.id ? <Loader size={14} className="animate-spin" /> : <Check size={14} />}
                  {zh ? '保存' : 'Save'}
                </button>
              </div>
              {detailTestResult && (
                <div
                  className={`add-provider-message ${detailTestResult.success ? 'success' : 'error'}`}
                  style={{ marginTop: '8px' }}
                >
                  {detailTestResult.success ? (
                    <>
                      <Check size={14} />
                      {zh ? '测试连接成功' : 'Connection test succeeded'}
                    </>
                  ) : (
                    detailTestResult.error || (zh ? '测试连接失败' : 'Connection test failed')
                  )}
                </div>
              )}
              {modelErrors[selected.id] && <p className="settings-error">{modelErrors[selected.id]}</p>}
            </section>

            <section className="settings-section-card">
              <div className="settings-section-heading settings-section-heading-row">
                <div><h4>API Keys</h4><p>{zh ? '测试连接后，可将可用的 Key 设为主 Key。' : 'Test a connection before setting a key as primary.'}</p></div>
                <span className="settings-count">{selected.keys.filter((key) => key.isActive).length} / {selected.keys.length}</span>
              </div>

              {selected.keys.length === 0 ? (
                <div className="provider-key-empty"><KeyRound size={18} />{zh ? '还没有 API Key' : 'No API keys yet'}</div>
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
                          <span>{testedAt ? new Date(testedAt).toLocaleDateString(locale === 'zh' ? 'zh-CN' : 'en-US', { month: 'short', day: 'numeric' }) : (zh ? '尚未测试' : 'Not tested')}</span>
                        </div>
                        <div className="provider-key-actions">
                          <button type="button" onClick={() => testKey(key)} disabled={testing} className="settings-icon-button" title={zh ? '测试连接' : 'Test connection'}>{testing ? <Loader size={14} className="animate-spin" /> : <Wifi size={14} />}</button>
                          {!key.isPrimary && <button type="button" onClick={() => onSetPrimaryKey(selected.id, key.id)} disabled={key.status !== 'valid'} className="settings-icon-button" title={key.status !== 'valid' ? (zh ? '请先测试连接' : 'Test first') : t(locale, 'assistant.setPrimaryKey')}><Star size={14} /></button>}
                          {!key.isPrimary && <button type="button" onClick={() => setDeleteTarget({ providerId: selected.id, keyId: key.id })} className="settings-icon-button danger" title={zh ? '删除 Key' : 'Delete key'}><Trash2 size={14} /></button>}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}

              <div className="provider-add-key">
                <div className="provider-add-key-title"><Plus size={14} />{zh ? '添加新 Key' : 'Add key'}</div>
                <div className="provider-add-key-fields">
                  <input className="settings-input" value={newLabel} onChange={(event) => setNewLabel(event.target.value)} placeholder={zh ? '名称（例如：工作账号）' : 'Label (e.g. Work)'} />
                  <input className="settings-input" type="password" value={newKey} onChange={(event) => setNewKey(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && addKey()} placeholder={zh ? '粘贴 API Key' : 'Paste API key'} />
                  <button type="button" className="btn" onClick={addKey} disabled={!newKey.trim() || addingKey === selected.id}>{addingKey === selected.id ? <Loader size={14} className="animate-spin" /> : <Plus size={14} />}{zh ? '添加' : 'Add'}</button>
                </div>
                {addError && <p className="settings-error">{addError}</p>}
              </div>
            </section>
          </div>
        </div>
      )}

      <ConfirmDialog open={deleteTarget !== null} title={zh ? '删除 Key' : 'Delete key'} message={zh ? '确定删除这个 API Key？此操作不可撤销。' : 'Delete this API key? This action cannot be undone.'} confirmLabel={zh ? '删除' : 'Delete'} cancelLabel={zh ? '取消' : 'Cancel'} danger onConfirm={() => { if (deleteTarget) onDeleteKey(deleteTarget.providerId, deleteTarget.keyId); setDeleteTarget(null); }} onCancel={() => setDeleteTarget(null)} />
    </section>
  );
}
