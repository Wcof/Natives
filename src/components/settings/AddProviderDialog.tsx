'use client';

import { useState, useMemo } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { PROVIDER_PRESETS } from '@/lib/provider-presets';
import type { ProviderPreset } from '@/types/provider';
import { Search, Globe, Link, Key, Check, Plus, Trash2, Wifi, Loader } from 'lucide-react';
import { t as tr } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';

const TEST_MODELS = ['sensenova-6.7-flash-lite', 'deepseek-v4-flash'];

interface AddProviderDialogProps {
  locale: string;
  onClose: () => void;
  onSave: (data: {
    presetName: string;
    name: string;
    websiteUrl: string;
    baseUrl: string;
    keys: { label: string; apiKey: string }[];
  }) => Promise<void>;
}

/** 按 locale 显示供应商名称 */
function presetDisplayName(preset: ProviderPreset, locale: string): string {
  if (locale.startsWith('zh') && preset.nameZh) return preset.nameZh;
  return preset.name;
}

/** 按 locale 显示供应商描述 */
function presetDescription(preset: ProviderPreset, locale: string): string {
  if (locale.startsWith('zh') && preset.descriptionZh) return preset.descriptionZh;
  return preset.description || '';
}

export default function AddProviderDialog({ locale, onClose, onSave }: AddProviderDialogProps) {
  const [search, setSearch] = useState('');
  const [selected, setSelected] = useState<ProviderPreset | null>(null);
  const [name, setName] = useState('');
  const [websiteUrl, setWebsiteUrl] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [keys, setKeys] = useState<{ label: string; apiKey: string }[]>([{ label: 'API Key 1', apiKey: '' }]);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; error?: string } | null>(null);
  const [selectedModel, setSelectedModel] = useState(TEST_MODELS[0]);

  const filtered = useMemo(() => {
    if (!search.trim()) return PROVIDER_PRESETS;
    const q = search.toLowerCase();
    return PROVIDER_PRESETS.filter(p =>
      p.name.toLowerCase().includes(q) ||
      (p.nameZh?.toLowerCase() ?? '').includes(q) ||
      (p.description?.toLowerCase() ?? '').includes(q) ||
      (p.descriptionZh?.toLowerCase() ?? '').includes(q) ||
      p.websiteUrl.toLowerCase().includes(q)
    );
  }, [search]);

  const handleSelect = (preset: ProviderPreset) => {
    setSelected(preset);
    setName(presetDisplayName(preset, locale));
    setWebsiteUrl(preset.websiteUrl);
    setBaseUrl(preset.baseUrl);
    setKeys([{ label: 'API Key 1', apiKey: '' }]);
  };

  const handleSave = async () => {
    if (!name.trim() || !selected) return;
    const validKeys = keys.filter(k => k.apiKey.trim());
    if (validKeys.length === 0) return;
    setSaving(true);
    try {
      await onSave({
        presetName: selected.name,
        name: name.trim(),
        websiteUrl: websiteUrl.trim(),
        baseUrl: baseUrl.trim(),
        keys: validKeys,
      });
      onClose();
    } catch {
      setSaving(false);
    }
  };

  const addKeyRow = () => {
    setKeys(prev => [...prev, { label: `API Key ${prev.length + 1}`, apiKey: '' }]);
  };

  const removeKeyRow = (idx: number) => {
    setKeys(prev => prev.filter((_, i) => i !== idx));
  };

  const updateKey = (idx: number, field: 'label' | 'apiKey', value: string) => {
    setKeys(prev => prev.map((k, i) => (i === idx ? { ...k, [field]: value } : k)));
  };

  const t = (key: string) => tr(locale, key);
  const hasValidKey = keys.some(k => k.apiKey.trim().length > 0);
  const desc = selected ? presetDescription(selected, locale) : '';
  const isOpenAICompatible = selected?.name.includes('SenseNova') || selected?.name.toLowerCase().includes('openai');

  const handleTestConnection = async () => {
    if (!selected || !hasValidKey) return;
    setTesting(true);
    setTestResult(null);
    try {
      const firstKey = keys.find(k => k.apiKey.trim())!;
      const api = window.nativesAPI;
      
      // Use testRaw for unsaved providers (no DB key ID needed)
      if (api?.provider?.testRaw) {
        const result = await api.provider.testRaw({
          baseUrl: baseUrl.trim() || selected.baseUrl,
          apiKey: firstKey.apiKey.trim(),
          model: selectedModel,
        });
        setTestResult(result);
      } else if (api?.provider?.test) {
        // Fallback for saved providers
        const result = await api.provider.test({
          providerId: selected.name,
          keyId: firstKey.label,
          model: selectedModel,
        });
        setTestResult(result);
      } else {
        setTestResult({ success: false, error: 'Provider test not available in dev mode' });
      }
    } catch (err) {
      setTestResult({ success: false, error: classifyError(err).userMessage });
    } finally {
      setTesting(false);
    }
  };

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title={t('settings.addProvider')}
      width={700}
      contentClassName="!p-0 flex flex-col min-h-0 overflow-hidden"
    >
      {/* Search */}
      <div style={{ padding: `${SPACING.sm}px ${SPACING.lg}px`, borderBottom: '0.0625rem solid var(--border)' }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: SPACING.sm,
          padding: `${SPACING.xs}px ${SPACING.sm}px`,
          borderRadius: BORDER_RADIUS.md,
          background: 'var(--surface)', border: '0.0625rem solid var(--border)',
        }}>
          <Search size={14} style={{ color: 'var(--text-disabled)' }} />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t('settings.searchProvider')}
            style={{
              flex: 1, background: 'none', border: 'none', outline: 'none',
              color: 'var(--text)', fontSize: FONT_SIZE.sm,
            }}
          />
        </div>
      </div>

      {/* Body */}
      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        {/* Left: Preset list */}
        <div style={{ flex: 1, overflow: 'auto', padding: `${SPACING.sm}px` }}>
          {filtered.length === 0 ? (
            <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-disabled)', fontSize: FONT_SIZE.sm }}>
              {t('settings.noProviderMatch')}
            </div>
          ) : (
            filtered.map((preset) => {
              const isSelected = selected?.name === preset.name;
              const displayName = presetDisplayName(preset, locale);
              const displayDesc = presetDescription(preset, locale);
              return (
                <div
                  key={preset.name}
                  onClick={() => handleSelect(preset)}
                  style={{
                    display: 'flex', alignItems: 'center', gap: SPACING.sm,
                    padding: `${SPACING.sm}px ${SPACING.md}px`,
                    borderRadius: BORDER_RADIUS.lg, cursor: 'pointer',
                    background: isSelected
                      ? 'linear-gradient(135deg, var(--primary-soft) 0%, color-mix(in srgb, var(--primary-soft) 80%, transparent) 100%)'
                      : 'transparent',
                    border: isSelected
                      ? '0.0625rem solid var(--primary)'
                      : '0.0625rem solid transparent',
                    transition: 'all 0.12s',
                    marginBottom: 3,
                  }}
                  onMouseEnter={(e) => { if (!isSelected) (e.currentTarget as HTMLElement).style.background = 'var(--surface)'; }}
                  onMouseLeave={(e) => { if (!isSelected) (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
                >
                  {/* Color dot */}
                  <div style={{
                    width: 8, height: 8, borderRadius: '50%', flexShrink: 0,
                    background: preset.iconColor || 'var(--primary)',
                  }} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{
                      fontSize: FONT_SIZE.sm, fontWeight: isSelected ? 600 : 400,
                      color: 'var(--text)',
                    }}>
                      {displayName}
                    </div>
                    {displayDesc && (
                      <div style={{
                        fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)',
                        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                        marginTop: 1,
                      }}>
                        {displayDesc}
                      </div>
                    )}
                  </div>
                  {preset.category && (
                    <span style={{
                      fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)',
                      background: 'var(--surface)', padding: '1px 5px',
                      borderRadius: BORDER_RADIUS.sm, textTransform: 'uppercase', flexShrink: 0,
                    }}>
                      {preset.category}
                    </span>
                  )}
                  {isSelected && (
                    <Check size={14} style={{ color: 'var(--primary)', flexShrink: 0 }} />
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Right: Form */}
        <div style={{
          width: 300, flexShrink: 0,
          borderLeft: '0.0625rem solid var(--border)',
          padding: `${SPACING.md}px ${SPACING.lg}px`,
          overflow: 'auto',
          background: 'var(--surface)',
        }}>
          {selected ? (
            <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
              <h3 style={{ fontSize: FONT_SIZE.md, fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.xs }}>
                {presetDisplayName(selected, locale)}
              </h3>

              {desc && (
                <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', marginBottom: SPACING.xs, lineHeight: 1.4 }}>
                  {desc}
                </div>
              )}

              <Field label={t('settings.providerName')} icon={<Globe size={12} />}>
                <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} />
              </Field>

              <Field label={t('settings.websiteUrl')} icon={<Link size={12} />}>
                <input value={websiteUrl} onChange={(e) => setWebsiteUrl(e.target.value)} style={inputStyle} />
              </Field>

              <Field label={t('settings.baseUrl')} icon={<Link size={12} />}>
                <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} style={inputStyle} />
              </Field>

              {/* Multiple API Keys */}
              <div style={{ marginTop: SPACING.xs }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
                  <Field label="API Keys" icon={<Key size={12} />}>
                    <span />
                  </Field>
                  <button
                    onClick={addKeyRow}
                    style={{
                      background: 'none', border: 'none', color: 'var(--primary)',
                      cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 3,
                      fontSize: FONT_SIZE.xs, padding: 0,
                    }}
                  >
                    <Plus size={12} /> {t('common.add')}
                  </button>
                </div>

                {keys.map((k, idx) => (
                  <div key={idx} style={{ display: 'flex', gap: 4, marginBottom: 6, alignItems: 'flex-start' }}>
                    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                      <input
                        value={k.label}
                        onChange={(e) => updateKey(idx, 'label', e.target.value)}
                        placeholder={t('settings.keyLabel')}
                        style={{ ...inputStyle, fontSize: FONT_SIZE.xs, padding: '3px 6px' }}
                      />
                      <input
                        value={k.apiKey}
                        onChange={(e) => updateKey(idx, 'apiKey', e.target.value)}
                        type="password"
                        placeholder="sk-..."
                        style={{ ...inputStyle, fontSize: FONT_SIZE.xs, padding: '3px 6px' }}
                      />
                    </div>
                    {keys.length > 1 && (
                      <button
                        onClick={() => removeKeyRow(idx)}
                        style={{
                          background: 'none', border: 'none', color: 'var(--danger)',
                          cursor: 'pointer', padding: '6px 2px', flexShrink: 0,
                        }}
                        title={t('settings.remove')}
                      >
                        <Trash2 size={12} />
                      </button>
                    )}
                  </div>
                ))}
              </div>

              {/* OpenAI-compatible: model picker + test connection */}
              <div style={{ marginTop: SPACING.sm, display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
                {/* Show derived endpoints (read-only) for SenseNova */}
                {selected.name.includes('SenseNova') && (
                  <div style={{
                    padding: `${SPACING.xs}px ${SPACING.sm}px`,
                    background: 'var(--surface-hover)', borderRadius: BORDER_RADIUS.sm,
                    fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)',
                  }}>
                    <div>Models: <span style={{ color: 'var(--text)' }}>{baseUrl.trim() || '—'}/models</span></div>
                    <div>Chat: <span style={{ color: 'var(--text)' }}>{baseUrl.trim() || '—'}/chat/completions</span></div>
                  </div>
                )}

                {/* Model picker for OpenAI-compatible testing */}
                {selected.name.includes('SenseNova') && (
                  <div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 3 }}>
                      <span style={{ color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs }}>Test Model</span>
                    </div>
                    <select
                      value={selectedModel}
                      onChange={(e) => setSelectedModel(e.target.value)}
                      style={inputStyle}
                    >
                      {TEST_MODELS.map(m => (
                        <option key={m} value={m}>{m}</option>
                      ))}
                    </select>
                  </div>
                )}

                {/* Test connection button */}
                <button
                  onClick={handleTestConnection}
                  disabled={testing || !hasValidKey}
                  style={{
                    display: 'inline-flex', alignItems: 'center', gap: 6, justifyContent: 'center',
                    padding: '6px 12px', borderRadius: BORDER_RADIUS.md,
                    background: 'var(--surface)', border: '1px solid var(--border)',
                    color: 'var(--text)', cursor: testing ? 'not-allowed' : 'pointer',
                    fontSize: FONT_SIZE.xs, transition: 'all 0.12s',
                    opacity: testing ? 0.6 : 1,
                  }}
                >
                  {testing ? (
                    <Loader size={12} className="animate-spin" />
                  ) : (
                    <Wifi size={12} />
                  )}
                  {testing ? 'Testing...' : 'Test Connection'}
                </button>

                {/* Test result feedback */}
                {testResult && (
                  <div style={{
                    padding: `${SPACING.xs}px ${SPACING.sm}px`,
                    borderRadius: BORDER_RADIUS.sm,
                    background: testResult.success ? 'var(--primary-soft)' : 'rgba(239, 68, 68, 0.08)',
                    border: `1px solid ${testResult.success ? 'var(--success)' : 'var(--danger)'}`,
                    fontSize: FONT_SIZE.xs,
                    color: testResult.success ? 'var(--success)' : 'var(--danger)',
                  }}>
                    {testResult.success ? '✓ Connection successful' : `✗ ${testResult.error || 'Connection failed'}`}
                  </div>
                )}
              </div>

              <button
                onClick={handleSave}
                disabled={saving || !name.trim() || !hasValidKey}
                className="btn btn-primary"
                style={{ marginTop: SPACING.sm, fontSize: FONT_SIZE.sm, padding: '7px 0' }}
              >
                {saving ? t('common.saving') : t('common.save')}
              </button>
            </div>
          ) : (
            <div style={{ textAlign: 'center', padding: SPACING.xl, color: 'var(--text-disabled)', fontSize: FONT_SIZE.sm }}>
              {t('settings.selectProviderHint')}
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}

// ── Sub-components ──

function Field({ label, icon, children }: { label: string; icon: React.ReactNode; children: React.ReactNode }) {
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 3 }}>
        <span style={{ color: 'var(--text-disabled)', display: 'inline-flex' }}>{icon}</span>
        <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)' }}>{label}</span>
      </div>
      {children}
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  width: '100%', boxSizing: 'border-box',
  padding: '5px 8px', borderRadius: BORDER_RADIUS.sm,
  border: '0.0625rem solid var(--border)',
  background: 'var(--background)', color: 'var(--text)',
  fontSize: FONT_SIZE.xs, outline: 'none', fontFamily: 'var(--font-mono)',
};
