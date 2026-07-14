'use client';

import { useState, useMemo } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { PROVIDER_PRESETS } from '@/lib/provider-presets';
import { Search, Globe, Link, Key, Check, Wifi, Loader, RefreshCw } from 'lucide-react';
import { t as tr } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';
import { canTestDiscoveredModel, connectionFingerprint, normalizeDiscoveredModels, selectDiscoveredModel } from '@/lib/provider-model-selection';

interface Props { locale: string; onClose: () => void; onSave: (data: { presetName: string; name: string; websiteUrl: string; baseUrl: string; keys: { label: string; apiKey: string }[] }) => Promise<void>; }

export default function AddProviderDialog({ locale, onClose, onSave }: Props) {
  const [search, setSearch] = useState('');
  const [sel, setSel] = useState<any | null>(null);
  const [name, setName] = useState(''); const [wu, setWu] = useState(''); const [bu, setBu] = useState('');
  const [dm, setDm] = useState(''); const [mos, setMos] = useState<Array<{ id: string; displayName?: string }>>([]);
  const [fm, setFm] = useState(false); const [fme, setFme] = useState<string | null>(null);
  const [sv, setSv] = useState(false); const [te, setTe] = useState(false);
  const [trr, setTrr] = useState<{ success: boolean; error?: string } | null>(null);
  const [sve, setSve] = useState<string | null>(null); const [dfp, setDfp] = useState<string | null>(null);
  const [kl, setKl] = useState('API Key 1'); const [kv, setKv] = useState('');

  const fil = useMemo(() => {
    if (!search.trim()) return PROVIDER_PRESETS;
    const q = search.toLowerCase();
    return PROVIDER_PRESETS.filter((p: any) => p.name.toLowerCase().includes(q) || (p.nameZh?.toLowerCase() ?? '').includes(q) || (p.description?.toLowerCase() ?? '').includes(q) || (p.descriptionZh?.toLowerCase() ?? '').includes(q) || p.websiteUrl.toLowerCase().includes(q));
  }, [search]);

  const hs = (preset: any) => { setSel(preset); setName(pn(preset, locale)); setWu(preset.websiteUrl); setBu(preset.baseUrl); setDm(''); setKl('API Key 1'); setKv(''); setMos([]); setFme(null); setDfp(null); setTrr(null); setSve(null); };
  const hsv = async () => {
    if (!name.trim() || !sel) return;
    if (!dm.trim()) { setSve('Default model is required.'); return; }
    if (!kv.trim()) { setSve('An API key is required.'); return; }
    if (!trr?.success) { setSve('Test the connection before saving.'); return; }
    setSv(true); setSve(null);
    try { await onSave({ presetName: sel.name, name: name.trim(), websiteUrl: wu.trim(), baseUrl: bu.trim(), keys: [{ label: kl.trim() || 'API Key 1', apiKey: kv }] }); setKv(''); onClose(); }
    catch (e) { setSve(classifyError(e).userMessage); setSv(false); }
  };

  const t = (k: string) => tr(locale, k);
  const hk = kv.trim().length > 0; const desc = sel ? pd(sel, locale) : '';
  const fp = connectionFingerprint(bu, kv); const dc = dfp === fp;
  const cum = dc && canTestDiscoveredModel(mos);
  const cs = !sv && name.trim().length > 0 && hk && cum && trr?.success === true;
  const inv = () => { setMos([]); setDm(''); setFme(null); setDfp(null); setTrr(null); };
  const htt = async () => { if (!sel || !hk || !cum) return; setTe(true); setTrr(null); try { const a = window.nativesAPI; if (a?.provider?.testCandidate) { const r = await a.provider.testCandidate({ providerType: 'openai', baseUrl: bu.trim() || sel.baseUrl, apiKey: kv.trim(), model: dm }); setTrr({ success: r.success, error: r.userMessage ?? undefined }); } else setTrr({ success: false, error: t('settings.providerTestUnavailable') }); } catch (e) { setTrr({ success: false, error: classifyError(e).userMessage }); } finally { setTe(false); } };
  const hd = async () => {
    if (!sel || !hk || fm) return; setFm(true); setFme(null);
    try {
      const a = window.nativesAPI?.provider;
      if (!a?.discoverModels) { setFme(t('settings.modelDiscoveryUnavailable')); return; }
      const ms = normalizeDiscoveredModels(await a.discoverModels({ providerType: 'openai', baseUrl: bu.trim() || sel.baseUrl, apiKey: kv.trim() }));
      setMos(ms); const m = selectDiscoveredModel(ms); if (m) setDm(m.id);
      setDfp(connectionFingerprint(bu.trim() || sel.baseUrl, kv.trim())); setTrr(null);
      if (ms.length === 0) setFme(t('settings.noModelsDiscovered'));
    } catch (e) { setFme(classifyError(e).userMessage); } finally { setFm(false); }
  };

  return (
    <Modal isOpen={true} onClose={onClose} title={t('settings.addProvider')} width={700} contentClassName="!p-0 flex flex-col min-h-0 overflow-hidden">
      <div style={{ padding: `${SPACING.sm}px ${SPACING.lg}px`, borderBottom: '0.0625rem solid var(--border)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.md, background: 'var(--surface)', border: '0.0625rem solid var(--border)' }}>
          <Search size={14} style={{ color: 'var(--text-disabled)' }} />
          <input value={search} onChange={e => setSearch(e.target.value)} placeholder={t('settings.searchProvider')} style={{ flex: 1, background: 'none', border: 'none', outline: 'none', color: 'var(--text)', fontSize: FONT_SIZE.sm }} />
        </div>
      </div>
      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        <div style={{ flex: 1, overflow: 'auto', padding: `${SPACING.sm}px` }}>
          {fil.length === 0 ? <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-disabled)', fontSize: FONT_SIZE.sm }}>{t('settings.noProviderMatch')}</div>
          : fil.map((preset: any) => {
            const s = sel?.name === preset.name;
            return <div key={preset.name} onClick={() => hs(preset)} style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, padding: `${SPACING.sm}px ${SPACING.md}px`, borderRadius: BORDER_RADIUS.lg, cursor: 'pointer', background: s ? 'linear-gradient(135deg, var(--primary-soft) 0%, color-mix(in srgb, var(--primary-soft) 80%, transparent) 100%)' : 'transparent', border: s ? '0.0625rem solid var(--primary)' : '0.0625rem solid transparent', transition: 'all 0.12s', marginBottom: 3 }}>
              <div style={{ width: 8, height: 8, borderRadius: '50%', flexShrink: 0, background: preset.iconColor || 'var(--primary)' }} />
              <div style={{ flex: 1, minWidth: 0 }}><div style={{ fontSize: FONT_SIZE.sm, fontWeight: s ? 600 : 400, color: 'var(--text)' }}>{pn(preset, locale)}</div>{pd(preset, locale) && <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', marginTop: 1 }}>{pd(preset, locale)}</div>}</div>
              {preset.category && <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', background: 'var(--surface)', padding: '1px 5px', borderRadius: BORDER_RADIUS.sm, textTransform: 'uppercase' }}>{preset.category}</span>}
              {s && <Check size={14} style={{ color: 'var(--primary)', flexShrink: 0 }} />}
            </div>;
          })}
        </div>
        <div style={{ width: 300, flexShrink: 0, borderLeft: '0.0625rem solid var(--border)', padding: `${SPACING.md}px ${SPACING.lg}px`, overflow: 'auto', background: 'var(--surface)' }}>
          {sel ? <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
            <h3 style={{ fontSize: FONT_SIZE.md, fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.xs }}>{pn(sel, locale)}</h3>
            {desc && <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', lineHeight: 1.4 }}>{desc}</div>}
            <Field label="Name" icon={<Globe size={12} />}><input value={name} onChange={e => setName(e.target.value)} style={inp} /></Field>
            <Field label="Website URL" icon={<Link size={12} />}><input value={wu} onChange={e => setWu(e.target.value)} style={inp} /></Field>
            <Field label="Base URL" icon={<Link size={12} />}><input value={bu} onChange={e => { setBu(e.target.value); inv(); }} style={inp} /></Field>
            <Field label="Initial API Key" icon={<Key size={12} />}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                <input value={kl} onChange={e => setKl(e.target.value)} placeholder="Label" style={inp} />
                <input value={kv} onChange={e => { setKv(e.target.value); if (dfp) inv(); }} type="password" placeholder="sk-..." style={inp} />
              </div>
            </Field>
            <Field label="Default Model" icon={<Link size={12} />}>
              <div style={{ display: 'flex', gap: SPACING.xs }}>
                <select value={dm} onChange={e => setDm(e.target.value)} style={{ ...inp, flex: 1 }} disabled={mos.length === 0 || !dc}>
                  {mos.length === 0 && <option value="">Fetch models first</option>}
                  {mos.map(m => <option key={m.id} value={m.id}>{m.displayName ?? m.id}</option>)}
                </select>
                <button type="button" onClick={hd} disabled={!hk || fm} title="Fetch" style={{ ...inp, width: 'auto', display: 'inline-flex', alignItems: 'center', gap: 4 }}>{fm ? <Loader size={13} className="animate-spin" /> : <RefreshCw size={13} />} Fetch</button>
              </div>
              {mos.length > 0 && <div style={{ marginTop: 4, color: 'var(--success)', fontSize: FONT_SIZE.xs }}>{mos.length} models found</div>}
              {fme && <div style={{ marginTop: 4, color: 'var(--danger)', fontSize: FONT_SIZE.xs }}>{fme}</div>}
            </Field>
            <div style={{ marginTop: SPACING.xs, display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
              <button onClick={htt} disabled={te || !hk || !cum} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, justifyContent: 'center', padding: '6px 12px', borderRadius: BORDER_RADIUS.md, background: 'var(--surface)', border: '1px solid var(--border)', color: 'var(--text)', cursor: (te || !cum) ? 'not-allowed' : 'pointer', fontSize: FONT_SIZE.xs, opacity: te ? 0.6 : 1 }}>
                {te ? <Loader size={12} className="animate-spin" /> : <Wifi size={12} />}{te ? 'Testing…' : 'Test Connection'}
              </button>
              {trr && <div style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: BORDER_RADIUS.sm, background: trr.success ? 'var(--primary-soft)' : 'rgba(239, 68, 68, 0.08)', border: `1px solid ${trr.success ? 'var(--success)' : 'var(--danger)'}`, fontSize: FONT_SIZE.xs, color: trr.success ? 'var(--success)' : 'var(--danger)' }}>
                {trr.success ? '✓ Connection successful' : `✗ ${trr.error || 'Connection failed'}`}
              </div>}
            </div>
            {sve && <div style={{ color: 'var(--danger)', fontSize: FONT_SIZE.xs }}>{sve}</div>}
            <button onClick={hsv} disabled={!cs} className="btn btn-primary" style={{ marginTop: SPACING.sm, fontSize: FONT_SIZE.sm, padding: '7px 0' }}>{sv ? 'Saving…' : 'Save'}</button>
          </div> : <div style={{ textAlign: 'center', padding: SPACING.xl, color: 'var(--text-disabled)', fontSize: FONT_SIZE.sm }}>Select a provider from the left</div>}
        </div>
      </div>
    </Modal>
  );
}

function pn(p: any, l: string) { return (l.startsWith('zh') && p.nameZh) ? p.nameZh : p.name; }
function pd(p: any, l: string) { return (l.startsWith('zh') && p.descriptionZh) ? p.descriptionZh : (p.description || ''); }
function Field({ label, icon, children }: { label: string; icon: React.ReactNode; children: React.ReactNode }) {
  return <div><div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 3 }}><span style={{ color: 'var(--text-disabled)', display: 'inline-flex' }}>{icon}</span><span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)' }}>{label}</span></div>{children}</div>;
}
const inp: React.CSSProperties = { width: '100%', boxSizing: 'border-box', padding: '5px 8px', borderRadius: BORDER_RADIUS.sm, border: '0.0625rem solid var(--border)', background: 'var(--background)', color: 'var(--text)', fontSize: FONT_SIZE.xs, outline: 'none', fontFamily: 'var(--font-mono)' };
