'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FilePlus2, FileUp, RefreshCw, Search, Trash2, Users } from 'lucide-react';
import { BORDER_RADIUS, FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import type { ProviderSummary } from '@/types/provider';
import type { ProviderRoutingApi, Sub2ApiAccountSummary, Sub2ApiImportPreview } from '@/types/provider-routing';
import { selectionAfterDelete, toggleAccountSelection } from './accountSelection';

export function Sub2ApiAccountPool({ locale, providers, api, onProviderCreated }: { locale: Locale; providers: ProviderSummary[]; api: ProviderRoutingApi | undefined; onProviderCreated: () => Promise<void> }) {
  const { toast } = useToast();
  const pools = useMemo(() => providers.filter((provider) => provider.providerType === 'sub2api'), [providers]);
  const [providerId, setProviderId] = useState('');
  const [accounts, setAccounts] = useState<Sub2ApiAccountSummary[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [content, setContent] = useState('');
  const [preview, setPreview] = useState<Sub2ApiImportPreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [query, setQuery] = useState('');
  // T216 (P1-020): unified create-pool name dialog replaces window.prompt.
  const [showCreatePool, setShowCreatePool] = useState(false);
  const [poolNameDraft, setPoolNameDraft] = useState('');
  const fileInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!providerId && pools[0]) setProviderId(pools[0].id);
  }, [pools, providerId]);

  const load = useCallback(async () => {
    if (!providerId || !api?.listSub2ApiAccounts) return;
    setLoading(true); setError(null);
    try { setAccounts(await api.listSub2ApiAccounts(providerId)); setSelected(new Set()); }
    catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setLoading(false); }
  }, [api, locale, providerId]);

  useEffect(() => { void load(); }, [load]);

  const previewImport = async () => {
    if (!providerId || !api?.previewSub2ApiImport || !content.trim()) return;
    setBusy(true); setError(null);
    try { setPreview(await api.previewSub2ApiImport({ providerId, content: content.trim() })); }
    catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setBusy(false); }
  };
  const commitImport = async () => {
    if (!providerId || !api?.commitSub2ApiImport || !content.trim()) return;
    setBusy(true); setError(null);
    try {
      const result = await api.commitSub2ApiImport({ providerId, content: content.trim() });
      toast(t(locale, 'settings.sub2apiImportSuccess', { created: result.created, updated: result.updated, skipped: result.skipped }), 'success');
      setContent(''); setPreview(null); await load();
    } catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setBusy(false); }
  };
  const deleteAccounts = async () => {
    if (!providerId || !api?.deleteSub2ApiAccounts || selected.size === 0) return;
    setBusy(true);
    try {
      const result = await api.deleteSub2ApiAccounts({ providerId, accountIds: [...selected] });
      setAccounts((current) => current.filter((account) => !result.deleted.includes(account.id)));
      setSelected((current) => selectionAfterDelete(current, result.deleted));
      toast(t(locale, 'settings.sub2apiDeleteSuccess', { count: result.deleted.length }), 'success');
    } catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setBusy(false); setConfirmDelete(false); }
  };

  const createPool = async () => {
    const name = poolNameDraft.trim();
    if (!name || !api?.createSub2ApiPool) return;
    setBusy(true); setError(null);
    try { await api.createSub2ApiPool({ name }); await onProviderCreated(); toast(t(locale, 'settings.sub2apiPoolCreated'), 'success'); }
    catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setBusy(false); setShowCreatePool(false); setPoolNameDraft(''); }
  };
  const filteredAccounts = accounts.filter((account) => `${account.name} ${account.email ?? ''} ${account.platform} ${account.status}`.toLowerCase().includes(query.trim().toLowerCase()));
  const loadFiles = async (files: FileList | File[]) => {
    setContent((await Promise.all([...files].map((file) => file.text()))).join('\n'));
    setPreview(null);
  };
  if (pools.length === 0) return <section className="settings-section-card" style={{ marginTop: SPACING.lg }}>
    <div style={headerStyle}><span style={iconStyle}><Users size={18} /></span><div style={{ flex: 1 }}><h3 style={titleStyle}>{t(locale, 'settings.sub2apiAccountPool')}</h3><p style={descriptionStyle}>{t(locale, 'settings.sub2apiAccountPoolDesc')}</p></div><button type="button" className="btn" disabled={busy || !api?.createSub2ApiPool} onClick={() => void createPool()}><FilePlus2 size={14} /> {t(locale, 'settings.sub2apiCreatePool')}</button></div>{error && <div role="alert" style={errorStyle}>{error}</div>}
  </section>;
  if (!api?.listSub2ApiAccounts) return <div style={unavailableStyle}>{t(locale, 'settings.sub2apiUnavailable')}</div>;
  const allSelected = filteredAccounts.length > 0 && filteredAccounts.every((account) => selected.has(account.id));
  return <section className="settings-section-card" style={{ marginTop: SPACING.lg }}>
    <div style={headerStyle}><span style={iconStyle}><Users size={18} /></span><div style={{ flex: 1 }}><h3 style={titleStyle}>{t(locale, 'settings.sub2apiAccountPool')}</h3><p style={descriptionStyle}>{t(locale, 'settings.sub2apiAccountPoolDesc')}</p></div><button type="button" className="btn" onClick={() => void load()} disabled={loading || busy}><RefreshCw size={14} /> {t(locale, 'common.refresh')}</button></div>
    <label style={fieldStyle}>{t(locale, 'settings.sub2apiProvider')}<select value={providerId} onChange={(event) => setProviderId(event.target.value)} style={inputStyle}>{pools.map((pool) => <option key={pool.id} value={pool.id}>{pool.displayName}</option>)}</select></label>
    <div style={importStyle} onDragOver={(event) => event.preventDefault()} onDrop={(event) => { event.preventDefault(); void loadFiles(event.dataTransfer.files); }}><label style={fieldStyle}>{t(locale, 'settings.sub2apiImport')}<textarea value={content} onChange={(event) => { setContent(event.target.value); setPreview(null); }} placeholder={t(locale, 'settings.sub2apiImportPlaceholder')} style={textareaStyle} /></label><input ref={fileInput} type="file" accept="application/json,.json,.jsonl,.txt" multiple hidden onChange={(event) => event.target.files && void loadFiles(event.target.files)} /><div style={{ display: 'flex', gap: SPACING.sm }}><button type="button" className="btn" onClick={() => fileInput.current?.click()}><FileUp size={14} /> {t(locale, 'settings.sub2apiChooseFiles')}</button><button type="button" className="btn" disabled={busy || !content.trim()} onClick={() => void previewImport()}>{t(locale, 'settings.sub2apiPreview')}</button>{preview && <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void commitImport()}>{t(locale, 'settings.sub2apiImportCommit')}</button>}</div></div>
    {preview && <Preview locale={locale} preview={preview} />}
    {error && <div role="alert" style={errorStyle}>{error}</div>}
    <div style={tableToolbarStyle}><strong style={{ color: 'var(--text)', fontSize: FONT_SIZE.sm }}>{t(locale, 'settings.sub2apiAccounts')} · {accounts.length}</strong><span style={searchStyle}><Search size={13} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t(locale, 'settings.sub2apiFilter')} style={searchInputStyle} /></span><button type="button" className="btn" disabled={selected.size === 0 || busy} onClick={() => setConfirmDelete(true)} style={{ color: 'var(--danger)' }}><Trash2 size={14} /> {t(locale, 'settings.sub2apiDeleteSelected', { count: selected.size })}</button></div>
    {loading ? <div style={stateStyle}>{t(locale, 'common.loading')}</div> : accounts.length === 0 ? <div style={stateStyle}>{t(locale, 'settings.sub2apiNoAccounts')}</div> : <div style={tableWrapStyle}><table style={tableStyle}><thead><tr><th style={cellStyle}><input aria-label={t(locale, 'settings.sub2apiSelectAll')} type="checkbox" checked={allSelected} onChange={() => setSelected(allSelected ? new Set() : new Set(filteredAccounts.map((account) => account.id)))} /></th><th style={cellStyle}>{t(locale, 'settings.sub2apiAccount')}</th><th style={cellStyle}>{t(locale, 'settings.sub2apiPlatform')}</th><th style={cellStyle}>{t(locale, 'settings.sub2apiStatus')}</th><th style={cellStyle}>{t(locale, 'settings.sub2apiCapacity')}</th></tr></thead><tbody>{filteredAccounts.map((account) => <tr key={account.id}><td style={cellStyle}><input aria-label={account.name} type="checkbox" checked={selected.has(account.id)} onChange={() => setSelected((current) => toggleAccountSelection(current, account.id))} /></td><td style={cellStyle}><strong>{account.name}</strong>{account.email && <span style={secondaryStyle}>{account.email}</span>}{account.expiresAt && <span style={secondaryStyle}>{t(locale, 'settings.sub2apiExpiresAt')}: {new Date(account.expiresAt).toLocaleString()}</span>}</td><td style={cellStyle}>{account.platform} · {account.accountType}</td><td style={cellStyle}><span style={statusStyle(account.status)}>{t(locale, `settings.sub2apiStatus${account.status[0]!.toUpperCase()}${account.status.slice(1)}`)}</span></td><td style={cellStyle}>{account.concurrency} · {account.priority}</td></tr>)}</tbody></table></div>}
    <ConfirmDialog open={confirmDelete} title={t(locale, 'settings.sub2apiDeleteTitle')} message={t(locale, 'settings.sub2apiDeleteMessage', { count: selected.size })} confirmLabel={t(locale, 'common.delete')} cancelLabel={t(locale, 'common.cancel')} danger onConfirm={() => void deleteAccounts()} onCancel={() => setConfirmDelete(false)} />
    {/* T216 (P1-020): unified create-pool name dialog replaces window.prompt */}
    {showCreatePool && (
      <div role="dialog" aria-modal="true" style={{ position: 'fixed', inset: 0, zIndex: 1000, display: 'flex', alignItems: 'flex-start', justifyContent: 'center', paddingTop: '20vh', background: 'rgba(0,0,0,0.5)' }} onClick={(e) => { if (e.target === e.currentTarget) { setShowCreatePool(false); setPoolNameDraft(''); } }}>
        <div style={{ width: 360, maxWidth: '90vw', background: 'var(--panel, #0e0f0c)', border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.md, padding: SPACING.lg, display: 'grid', gap: SPACING.md }}>
          <strong style={{ color: 'var(--text)', fontSize: FONT_SIZE.md }}>{t(locale, 'settings.sub2apiCreatePool')}</strong>
          <input className="input" autoFocus value={poolNameDraft} onChange={(event) => setPoolNameDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') void createPool(); if (event.key === 'Escape') { setShowCreatePool(false); setPoolNameDraft(''); } }} placeholder={t(locale, 'settings.sub2apiPoolNamePrompt')} style={{ ...inputStyle, width: '100%' }} />
          <div style={{ display: 'flex', gap: SPACING.sm, justifyContent: 'flex-end' }}>
            <button type="button" className="btn" onClick={() => { setShowCreatePool(false); setPoolNameDraft(''); }}>{t(locale, 'common.cancel')}</button>
            <button type="button" className="btn btn-primary" disabled={busy || !poolNameDraft.trim()} onClick={() => void createPool()}>{t(locale, 'settings.sub2apiCreatePool')}</button>
          </div>
        </div>
      </div>
    )}
  </section>;
}

function Preview({ locale, preview }: { locale: Locale; preview: Sub2ApiImportPreview }) {
  return <div style={previewStyle}><strong>{t(locale, 'settings.sub2apiPreviewTitle')}</strong><span style={secondaryStyle}>{t(locale, 'settings.sub2apiPreviewSummary', { count: preview.items.length, rejected: preview.rejected })}</span>{preview.items.slice(0, 8).map((item) => <div key={item.index} style={previewRowStyle}><span>{item.name || t(locale, 'settings.sub2apiUnnamedAccount')}</span><span style={statusStyle(item.action === 'reject' ? 'invalid' : item.action === 'skip' ? 'paused' : 'active')}>{t(locale, `settings.sub2apiImport${item.action[0]!.toUpperCase()}${item.action.slice(1)}`)}</span></div>)}</div>;
}

const headerStyle: React.CSSProperties = { display: 'flex', alignItems: 'flex-start', gap: SPACING.md, marginBottom: SPACING.lg };
const iconStyle: React.CSSProperties = { display: 'inline-flex', padding: SPACING.sm, borderRadius: BORDER_RADIUS.sm, color: 'var(--primary)', background: 'var(--primary-soft)' };
const titleStyle: React.CSSProperties = { margin: 0, color: 'var(--text)', fontSize: FONT_SIZE.md };
const descriptionStyle: React.CSSProperties = { margin: `${SPACING.xs}px 0 0`, color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs, lineHeight: 1.5 };
const fieldStyle: React.CSSProperties = { display: 'grid', gap: SPACING.xs, color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs };
const inputStyle: React.CSSProperties = { height: 36, padding: `0 ${SPACING.sm}px`, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, background: 'var(--surface-hover)', color: 'var(--text)' };
const textareaStyle: React.CSSProperties = { minHeight: 92, resize: 'vertical', padding: SPACING.sm, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, background: 'var(--surface-hover)', color: 'var(--text)', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.xs };
const importStyle: React.CSSProperties = { display: 'grid', gap: SPACING.sm, marginTop: SPACING.lg, paddingTop: SPACING.lg, borderTop: '1px solid var(--border)' };
const tableToolbarStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: SPACING.md, marginTop: SPACING.lg };
const searchStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: SPACING.xs, height: 32, padding: `0 ${SPACING.sm}px`, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, color: 'var(--text-secondary)' };
const searchInputStyle: React.CSSProperties = { minWidth: 120, border: 0, outline: 0, background: 'transparent', color: 'var(--text)', fontSize: FONT_SIZE.xs };
const tableWrapStyle: React.CSSProperties = { overflowX: 'auto', marginTop: SPACING.sm, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm };
const tableStyle: React.CSSProperties = { width: '100%', borderCollapse: 'collapse', fontSize: FONT_SIZE.xs, color: 'var(--text)' };
const cellStyle: React.CSSProperties = { padding: `${SPACING.sm}px ${SPACING.md}px`, borderBottom: '1px solid var(--border)', textAlign: 'left', verticalAlign: 'middle' };
const secondaryStyle: React.CSSProperties = { display: 'block', marginTop: 2, color: 'var(--text-secondary)', fontSize: FONT_SIZE.micro };
const stateStyle: React.CSSProperties = { padding: SPACING.xl, color: 'var(--text-secondary)', fontSize: FONT_SIZE.sm, textAlign: 'center' };
const unavailableStyle: React.CSSProperties = { marginTop: SPACING.lg, padding: SPACING.lg, border: '1px dashed var(--border)', borderRadius: BORDER_RADIUS.md, color: 'var(--text-secondary)', fontSize: FONT_SIZE.sm };
const errorStyle: React.CSSProperties = { marginTop: SPACING.md, padding: SPACING.sm, border: '1px solid var(--danger)', borderRadius: BORDER_RADIUS.sm, color: 'var(--danger)', fontSize: FONT_SIZE.xs };
const previewStyle: React.CSSProperties = { display: 'grid', gap: SPACING.xs, marginTop: SPACING.md, padding: SPACING.md, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, background: 'var(--surface-hover)', fontSize: FONT_SIZE.xs };
const previewRowStyle: React.CSSProperties = { display: 'flex', justifyContent: 'space-between', gap: SPACING.md, color: 'var(--text)' };
function statusStyle(status: string): React.CSSProperties { const color = status === 'active' || status === 'create' || status === 'update' ? 'var(--success)' : status === 'expired' || status === 'invalid' || status === 'reject' ? 'var(--danger)' : 'var(--warning)'; return { display: 'inline-flex', width: 'fit-content', padding: '2px 6px', borderRadius: BORDER_RADIUS.pill, color, background: `color-mix(in srgb, ${color} 12%, transparent)`, fontSize: FONT_SIZE.micro, fontWeight: 600 }; }
