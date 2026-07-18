'use client';

import { useMemo, useState, type CSSProperties } from 'react';
import { Check, ChevronRight, KeyRound, Loader, RefreshCw, Search, Server, ShieldCheck, Wifi } from 'lucide-react';
import { CONFIGURABLE_PROVIDER_PRESETS } from '@/lib/provider-presets';
import { t } from '@/i18n';
import type { ApiProtocol, ProviderPreset } from '@/types/provider';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';
import { connectionFingerprint, normalizeDiscoveredModels, selectDiscoveredModel } from '@/lib/provider-model-selection';

interface SaveProviderInput {
  providerType: string;
  apiProtocol: ApiProtocol;
  name: string;
  websiteUrl: string;
  baseUrl: string;
  defaultModel: string;
  keys: { label: string; apiKey: string }[];
}

interface Props {
  locale: string;
  onClose: () => void;
  onSave: (data: SaveProviderInput) => Promise<void>;
}

interface DiscoveredModel {
  id: string;
  displayName?: string;
}

type ProviderColorStyle = CSSProperties & { '--provider-color': string };

function providerName(provider: ProviderPreset, locale: string) {
  return locale.startsWith('zh') && provider.nameZh ? provider.nameZh : provider.name;
}

function providerDescription(provider: ProviderPreset, locale: string) {
  return locale.startsWith('zh') && provider.descriptionZh
    ? provider.descriptionZh
    : provider.description ?? '';
}

export default function AddProviderDialog({ locale, onClose, onSave }: Props) {
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<ProviderPreset | null>(null);
  const [selectedProtocol, setSelectedProtocol] = useState<ApiProtocol>('openai_chat_completions');
  const [name, setName] = useState('');
  const [websiteUrl, setWebsiteUrl] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [keyLabel, setKeyLabel] = useState('API Key 1');
  const [apiKey, setApiKey] = useState('');
  const [models, setModels] = useState<DiscoveredModel[]>([]);
  const [defaultModel, setDefaultModel] = useState('');
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [discoveryFingerprint, setDiscoveryFingerprint] = useState<string | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; error?: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const filteredProviders = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return CONFIGURABLE_PROVIDER_PRESETS;
    return CONFIGURABLE_PROVIDER_PRESETS.filter((provider) =>
      `${provider.name} ${provider.nameZh ?? ''} ${provider.description ?? ''} ${provider.descriptionZh ?? ''}`
        .toLowerCase()
        .includes(needle),
    );
  }, [query]);

  const effectiveBaseUrl = baseUrl.trim();
  const effectiveApiKey = apiKey.trim();
  const currentFingerprint = `${selectedProtocol}:${connectionFingerprint(effectiveBaseUrl, effectiveApiKey)}`;
  const discoveryCurrent = discoveryFingerprint === currentFingerprint;
  const detailsReady = Boolean(name.trim() && effectiveBaseUrl && effectiveApiKey);
  const canTest = detailsReady && Boolean(defaultModel.trim());
  const canSave = detailsReady && canTest && testResult?.success === true && !saving;

  const invalidateConnection = () => {
    setModels([]);
    setDefaultModel('');
    setDiscoveryFingerprint(null);
    setDiscoveryError(null);
    setTestResult(null);
    setSaveError(null);
  };

  const selectProvider = (provider: ProviderPreset) => {
    setSelected(provider);
    setName(providerName(provider, locale));
    setWebsiteUrl(provider.websiteUrl);
    setBaseUrl(provider.baseUrl);
    setSelectedProtocol(provider.protocol ?? 'anthropic_messages');
    setKeyLabel('API Key 1');
    setApiKey('');
    invalidateConnection();
  };

  const discoverModels = async () => {
    if (!selected || !effectiveBaseUrl || !effectiveApiKey || discovering) return;
    setDiscovering(true);
    setDiscoveryError(null);
    setTestResult(null);
    try {
      const providerApi = window.nativesAPI?.provider;
      if (!providerApi?.discoverModels) throw new Error(t(locale, 'settings.modelDiscoveryUnavailable'));
      const discovered = normalizeDiscoveredModels(await providerApi.discoverModels({
        providerType: selectedProtocol,
        apiProtocol: selectedProtocol,
        baseUrl: effectiveBaseUrl,
        apiKey: effectiveApiKey,
      }));
      setModels(discovered);
      setDefaultModel(selectDiscoveredModel(discovered)?.id ?? '');
      setDiscoveryFingerprint(currentFingerprint);
      if (discovered.length === 0) setDiscoveryError(t(locale, 'settings.noModelsDiscovered'));
    } catch (error) {
      setModels([]);
      setDefaultModel('');
      setDiscoveryFingerprint(null);
      setDiscoveryError(classifyError(error).userMessage);
    } finally {
      setDiscovering(false);
    }
  };

  const testConnection = async () => {
    if (!selected || !canTest || testing) return;
    setTesting(true);
    setTestResult(null);
    try {
      const providerApi = window.nativesAPI?.provider;
      if (!providerApi?.testCandidate) throw new Error(t(locale, 'settings.providerTestUnavailable'));
      const result = await providerApi.testCandidate({
        providerType: selectedProtocol,
        apiProtocol: selectedProtocol,
        baseUrl: effectiveBaseUrl,
        apiKey: effectiveApiKey,
        model: defaultModel,
      });
      setTestResult({ success: result.success, error: result.userMessage ?? undefined });
    } catch (error) {
      setTestResult({ success: false, error: classifyError(error).userMessage });
    } finally {
      setTesting(false);
    }
  };

  const saveProvider = async () => {
    if (!selected) return;
    if (!effectiveApiKey) return setSaveError(t(locale, 'settings.apiKeyRequired'));
    if (!defaultModel) return setSaveError(t(locale, 'settings.defaultModelRequired'));
    if (!testResult?.success) return setSaveError(t(locale, 'settings.testBeforeSave'));
    setSaving(true);
    setSaveError(null);
    try {
      await onSave({
        providerType: selected.name,
        apiProtocol: selectedProtocol,
        name: name.trim(),
        websiteUrl: websiteUrl.trim(),
        baseUrl: effectiveBaseUrl,
        defaultModel,
        keys: [{ label: keyLabel.trim() || 'API Key 1', apiKey: effectiveApiKey }],
      });
      setApiKey('');
      onClose();
    } catch (error) {
      setSaveError(classifyError(error).userMessage);
      setSaving(false);
    }
  };

  return (
    <Modal isOpen onClose={onClose} title={t(locale, 'settings.addProvider')} width={920} contentClassName="!p-0 flex flex-col min-h-0 overflow-hidden">
      <div className="add-provider-intro"><ShieldCheck size={16} /><span>{t(locale, 'settings.providerSetupIntro')}</span></div>

      <div className="add-provider-layout">
        <aside className="add-provider-picker">
          <div className="add-provider-search"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t(locale, 'settings.searchProvider')} /></div>
          <div className="add-provider-options">
            {filteredProviders.map((provider) => (
              <button key={provider.name} type="button" className="add-provider-option" aria-pressed={selected?.name === provider.name} onClick={() => selectProvider(provider)}>
                <span className="add-provider-logo" style={{ '--provider-color': provider.iconColor ?? 'var(--primary)' } as ProviderColorStyle}>{providerName(provider, locale).slice(0, 1).toUpperCase()}</span>
                <span className="add-provider-option-copy"><strong>{providerName(provider, locale)}</strong><span>{providerDescription(provider, locale)}</span></span>
                <ChevronRight size={15} />
              </button>
            ))}
            {filteredProviders.length === 0 && <div className="add-provider-no-results">{t(locale, 'settings.noProviderMatch')}</div>}
          </div>
        </aside>

        <div className="add-provider-form-panel">
          {!selected ? (
            <div className="add-provider-empty"><span><Server size={24} /></span><strong>{t(locale, 'settings.selectProvider')}</strong><p>{t(locale, 'settings.selectProviderDesc')}</p></div>
          ) : (
            <>
              <div className="add-provider-steps" aria-label={t(locale, 'settings.connectionSetup')}>
                <span className={detailsReady ? 'complete' : 'active'}><b>1</b>{t(locale, 'settings.providerDetails')}</span>
                <span className={canTest ? 'complete' : detailsReady ? 'active' : ''}><b>2</b>{t(locale, 'settings.fetchModels')}</span>
                <span className={testResult?.success ? 'complete' : canTest ? 'active' : ''}><b>3</b>{t(locale, 'assistant.testConnection')}</span>
              </div>

              <div className="add-provider-form-scroll">
                <section className="add-provider-section">
                  <div className="add-provider-section-title"><span>1</span><div><h3>{t(locale, 'settings.providerDetails')}</h3><p>{t(locale, 'settings.providerDetailsDesc')}</p></div></div>
                  <div className="add-provider-field-grid">
                    <label><span>{t(locale, 'settings.providerName')}</span><input className="settings-input" value={name} onChange={(event) => setName(event.target.value)} placeholder={t(locale, 'settings.providerNamePlaceholder')} /></label>
                    <label><span>{t(locale, 'settings.websiteOptional')}</span><input className="settings-input" value={websiteUrl} onChange={(event) => setWebsiteUrl(event.target.value)} placeholder="https://example.com" /></label>
                    <label className="wide"><span>{t(locale, 'settings.baseUrl')}</span><input className="settings-input add-provider-mono" value={baseUrl} onChange={(event) => { setBaseUrl(event.target.value); invalidateConnection(); }} placeholder="https://api.example.com/v1" /></label>
                    <label><span>{locale.startsWith('zh') ? '协议' : 'Protocol'}</span><select className="settings-input" value={selectedProtocol} onChange={(event) => { setSelectedProtocol(event.target.value as ApiProtocol); invalidateConnection(); }}><option value="openai_chat_completions">OpenAI Chat Completions</option><option value="openai_responses">OpenAI Responses</option><option value="anthropic_messages">Anthropic Messages</option><option value="gemini_generate_content">Gemini Generate Content</option><option value="ollama_chat">Ollama Chat</option></select></label>
                  </div>
                </section>

                <section className="add-provider-section">
                  <div className="add-provider-section-title"><span>2</span><div><h3>{t(locale, 'settings.credential')}</h3><p>{t(locale, 'settings.credentialDesc')}</p></div></div>
                  <div className="add-provider-key-fields">
                    <label><span>{t(locale, 'settings.keyLabel')}</span><input className="settings-input" value={keyLabel} onChange={(event) => setKeyLabel(event.target.value)} placeholder={t(locale, 'settings.keyLabelPlaceholder')} /></label>
                    <label><span>API Key</span><div className="add-provider-key-input"><KeyRound size={15} /><input type="password" value={apiKey} onChange={(event) => { setApiKey(event.target.value); invalidateConnection(); }} placeholder={t(locale, 'settings.apiKeyPlaceholder')} /></div></label>
                  </div>
                </section>

                <section className="add-provider-section">
                  <div className="add-provider-section-title"><span>3</span><div><h3>{t(locale, 'settings.connectionSetup')}</h3><p>{t(locale, 'settings.connectionSetupDesc')}</p></div></div>
                  <div className="add-provider-connection-row">
                    <div style={{ position: 'relative', flex: 1, minWidth: 0 }}>
                      <input
                        className="settings-input"
                        value={defaultModel}
                        onChange={(event) => {
                          setDefaultModel(event.target.value);
                          setTestResult(null);
                        }}
                        onFocus={() => {
                          if (models.length > 0 && discoveryCurrent) setDropdownOpen(true);
                        }}
                        placeholder={locale.startsWith('zh') ? '输入或选择默认模型' : 'Enter or select default model'}
                        style={{ paddingRight: '32px' }}
                      />
                      {models.length > 0 && discoveryCurrent && (
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

                      {dropdownOpen && models.length > 0 && discoveryCurrent && (
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
                            {models.map((model) => (
                              <button
                                key={model.id}
                                type="button"
                                onClick={() => {
                                  setDefaultModel(model.id);
                                  setTestResult(null);
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
                    <button type="button" className="btn" onClick={discoverModels} disabled={!detailsReady || discovering}>{discovering ? <Loader size={14} className="animate-spin" /> : <RefreshCw size={14} />}{discovering ? t(locale, 'settings.fetchingModels') : t(locale, 'settings.fetchModels')}</button>
                    <button type="button" className="btn" onClick={testConnection} disabled={!canTest || testing}>{testing ? <Loader size={14} className="animate-spin" /> : <Wifi size={14} />}{t(locale, 'assistant.testConnection')}</button>
                  </div>
                  {models.length > 0 && discoveryCurrent && <div className="add-provider-success"><Check size={13} />{t(locale, 'settings.modelsFound', { count: models.length })}</div>}
                  {discoveryError && <div className="add-provider-message error">{discoveryError}</div>}
                  {testResult && <div className={`add-provider-message ${testResult.success ? 'success' : 'error'}`}>{testResult.success ? <><Check size={14} />{t(locale, 'assistant.testSuccess')}</> : testResult.error || t(locale, 'assistant.testFailed')}</div>}
                </section>
              </div>

              <footer className="add-provider-footer">
                <div>{saveError ? <span className="add-provider-footer-error">{saveError}</span> : !canSave ? t(locale, 'settings.completeProviderSteps') : <span className="add-provider-footer-ready"><Check size={13} />{t(locale, 'assistant.testSuccess')}</span>}</div>
                <div><button type="button" className="btn" onClick={onClose}>{t(locale, 'common.cancel')}</button><button type="button" className="btn btn-primary" onClick={saveProvider} disabled={!canSave}>{saving ? <Loader size={14} className="animate-spin" /> : <Check size={14} />}{saving ? t(locale, 'settings.savingProvider') : t(locale, 'settings.saveProvider')}</button></div>
              </footer>
            </>
          )}
        </div>
      </div>
    </Modal>
  );
}
