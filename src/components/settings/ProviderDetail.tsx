'use client';

import { useState, useMemo } from 'react';
import { Check, CircleAlert, Star, Plus, Search, Server, Trash2, Wifi, Loader, Ban, X, AlertCircle } from 'lucide-react';
import { BORDER_RADIUS, FONT_SIZE, SPACING, TRANSITION } from '@/lib/design-tokens';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import type { ProviderSummary, ProviderKeySummary, ProviderKeyStatus, TestKeyResult } from '@/types/provider';
import { classifyError } from '@/lib/error-classifier';
import ConfirmDialog from '@/components/ui/ConfirmDialog';

const SM: Record<ProviderKeyStatus, React.ReactNode> = {
  valid:        <Check size={10} style={{ color: 'var(--success)' }} />,
  invalid:      <X size={10} style={{ color: 'var(--danger)' }} />,
  rate_limited: <AlertCircle size={10} style={{ color: 'var(--warning)' }} />,
  unavailable:  <Ban size={10} style={{ color: 'var(--danger)' }} />,
  untested:     <CircleAlert size={10} style={{ color: 'var(--text-disabled)' }} />,
};

export default function ProviderDetail({ locale, providers, loading, showAddProvider, onSaveDefaults, onAddKey, onTestKey, onSetPrimaryKey, onDeleteKey, onDeleteProvider }: {
  locale: Locale; providers: ProviderSummary[]; loading: boolean;
  showAddProvider: () => void;
  onSaveDefaults: (pid: string, m: string | null) => Promise<void>;
  onAddKey: (pid: string, l: string, k: string) => Promise<void>;
  onTestKey: (pid: string, kid: string) => Promise<TestKeyResult>;
  onSetPrimaryKey: (pid: string, kid: string) => Promise<void>;
  onDeleteKey: (pid: string, kid: string) => Promise<void>;
  onDeleteProvider: (pid: string) => void;
}) {
  const [q, sq] = useState(''); const [sid, ss] = useState<string|null>(providers[0]?.id??null);
  const [md, smd] = useState<Record<string,string>>({}); const [sm_, ssm] = useState<string|null>(null);
  const [me, sme] = useState<Record<string,string>>({}); const [nl, snl] = useState(''); const [nk, snk] = useState('');
  const [ak, sak] = useState<string|null>(null); const [ae, sae] = useState<string|null>(null);
  const [tid, sti] = useState<string|null>(null); const [tl, stl] = useState(false);
  const [tr, str] = useState<Record<string,{s:ProviderKeyStatus;t:string;e:string|null}>>({});
  const [dt, sdt] = useState<{p:string;k:string}|null>(null);

  const fil = useMemo(() => { const n = q.trim().toLowerCase(); return !n ? providers : providers.filter(x => `${x.displayName} ${x.baseUrl}`.toLowerCase().includes(n)); }, [providers, q]);
  const sel = providers.find(x => x.id === sid) ?? fil[0] ?? null;
  if (sel && !(sel.id in md)) md[sel.id] = sel.defaultModel ?? '';

  if (loading) return (<section><h2 style={{fontSize:FONT_SIZE.lg,fontWeight:600,color:'var(--text)',margin:0}}>{t(locale,'settings.providers')}</h2><div style={{minHeight:160,display:'flex',alignItems:'center',justifyContent:'center',border:'1px dashed var(--border)',borderRadius:BORDER_RADIUS.md,marginTop:SPACING.lg}}><Loader size={22} className="animate-spin" style={{color:'var(--text-disabled)'}}/><span style={{marginLeft:8,color:'var(--text-secondary)',fontSize:FONT_SIZE.sm}}>{t(locale,'settings.providersLoading')}</span></div></section>);
  if (providers.length === 0) return (<section><div style={{display:'flex',justifyContent:'space-between',marginBottom:SPACING.lg}}><h2 style={{fontSize:FONT_SIZE.lg,fontWeight:600,color:'var(--text)',margin:0}}>{t(locale,'settings.providers')}</h2><button className="btn btn-primary" onClick={showAddProvider}><Plus size={15}/> {t(locale,'settings.addProvider')}</button></div><div style={{minHeight:160,display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',border:'1px dashed var(--border)',borderRadius:BORDER_RADIUS.md,padding:SPACING.xxl}}><Server size={24} style={{color:'var(--text-disabled)',marginBottom:SPACING.sm}}/><div style={{color:'var(--text)'}}>{t(locale,'settings.noProviders')}</div></div></section>);

  const hm = async () => { if (!sel) return; const v = md[sel.id]?.trim()??''; ssm(sel.id); sme(x=>({...x,[sel.id]:''})); try { await onSaveDefaults(sel.id, v||null); } catch(e) { sme(x=>({...x,[sel.id]:classifyError(e).userMessage})); } finally { ssm(null); } };
  const hk = async () => { if (!sel||!nk.trim()) return; sak(sel.id); sae(null); try { await onAddKey(sel.id, nl.trim()||`Key ${sel.keys.length+1}`, nk); snl(''); snk(''); } catch(e) { sae(classifyError(e).userMessage); } finally { sak(null); } };
  const ht = async (k: ProviderKeySummary) => { if (!sel||tl) return; stl(true); sti(k.id); try { const r = await onTestKey(sel.id,k.id); str(x=>({...x,[k.id]:{s:r.status,t:r.testedAt,e:r.userMessage}})); } catch(e) { str(x=>({...x,[k.id]:{s:'unavailable'as ProviderKeyStatus,t:new Date().toISOString(),e:classifyError(e).userMessage}})); } finally { sti(null); stl(false); } };

  return (
    <section>
      <div style={{display:'flex',justifyContent:'space-between',marginBottom:SPACING.lg}}>
        <h2 style={{fontSize:FONT_SIZE.lg,fontWeight:600,color:'var(--text)',margin:0}}>{t(locale,'settings.providers')}</h2>
        <button className="btn btn-primary" onClick={showAddProvider}><Plus size={15}/> {t(locale,'settings.addProvider')}</button>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'minmax(170px,0.3fr) minmax(0,1fr)',border:'1px solid var(--border)',borderRadius:BORDER_RADIUS.md,overflow:'hidden',minHeight:380}}>
        <aside style={{background:'var(--surface-hover)',borderRight:'1px solid var(--border)',padding:SPACING.md}}>
          <div style={{display:'flex',alignItems:'center',gap:8,height:34,padding:'0 9px',border:'1px solid var(--border)',borderRadius:BORDER_RADIUS.sm,background:'var(--surface)'}}>
            <Search size={14} style={{color:'var(--text-disabled)'}}/>
            <input value={q} onChange={e=>sq(e.target.value)} placeholder={t(locale,'settings.searchProvider')} style={{flex:1,border:0,outline:0,background:'transparent',color:'var(--text)',fontSize:FONT_SIZE.xs}}/>
          </div>
          <div style={{display:'flex',flexDirection:'column',gap:3,marginTop:SPACING.sm}}>{fil.map(x => {
            const is = sel?.id === x.id; const hp = x.keys.some(kk => kk.isPrimary);
            return (<button key={x.id} type="button" onClick={()=>{ss(x.id);sae(null);}} style={{width:'100%',display:'flex',alignItems:'center',gap:9,border:`1px solid ${is?'var(--primary)':'transparent'}`,borderRadius:BORDER_RADIUS.sm,background:is?'var(--primary-soft)':'transparent',padding:'8px',cursor:'pointer',transition:`background ${TRANSITION.normal}`}}>
              <span style={{width:6,height:6,borderRadius:'50%',flexShrink:0,background:hp?'var(--success)':'var(--text-disabled)'}}/>
              <span style={{flex:1,textAlign:'left'}}><span style={{display:'flex',alignItems:'center',gap:4,fontWeight:600,fontSize:FONT_SIZE.sm,color:'var(--text)'}}>{x.displayName}{hp&&<Star size={10} style={{color:'var(--warning)'}}/>}</span><span style={{display:'block',marginTop:2,color:'var(--text-disabled)',fontSize:FONT_SIZE.micro}}>{x.keys.length} keys</span></span>
            </button>);
          })}</div>
        </aside>
        {sel && <div style={{background:'var(--surface)',overflow:'auto'}}>
          <div style={{display:'flex',justifyContent:'space-between',padding:SPACING.xl,borderBottom:'1px solid var(--border)'}}>
            <div><div style={{display:'flex',alignItems:'center',gap:8}}><h3 style={{margin:0,fontSize:FONT_SIZE.lg,color:'var(--text)'}}>{sel.displayName}</h3>{sel.primaryKeyId&&<span style={{padding:'2px 6px',borderRadius:BORDER_RADIUS.xs,background:'var(--primary-soft)',color:'var(--primary)',fontSize:FONT_SIZE.micro,fontWeight:600}}>Primary</span>}</div><div style={{marginTop:4,color:'var(--text-disabled)',fontFamily:'var(--font-mono)',fontSize:FONT_SIZE.micro}}>{sel.baseUrl}</div></div>
            <button type="button" className="btn-ghost" onClick={()=>onDeleteProvider(sel.id)} style={{padding:6}}><Trash2 size={15}/></button>
          </div>
          <div style={{padding:`${SPACING.lg}px ${SPACING.xl}px`,borderBottom:'1px solid var(--border)'}}>
            <div style={{color:'var(--text-secondary)',fontSize:FONT_SIZE.xs,fontWeight:600,marginBottom:SPACING.sm}}>Provider Info</div>
            <div style={{display:'flex',flexDirection:'column',gap:SPACING.sm}}>
              <div><span style={{color:'var(--text-disabled)',fontSize:FONT_SIZE.xs,display:'block'}}>Name</span><span style={{color:'var(--text)',fontSize:FONT_SIZE.sm}}>{sel.displayName}</span></div>
              <div><span style={{color:'var(--text-disabled)',fontSize:FONT_SIZE.xs,display:'block'}}>Base URL</span><span style={{color:'var(--text)',fontSize:FONT_SIZE.sm}}>{sel.baseUrl}</span></div>
              <div style={{display:'flex',alignItems:'flex-end',gap:SPACING.sm}}>
                <div style={{flex:1}}>
                  <span style={{color:'var(--text-disabled)',fontSize:FONT_SIZE.xs,display:'block',marginBottom:3}}>Default Model</span>
                  <input value={md[sel.id]??sel.defaultModel??''} onChange={e=>smd(x=>({...x,[sel.id]:e.target.value}))} placeholder="gpt-4o" style={{height:36,padding:'0 10px',border:'1px solid var(--border)',borderRadius:BORDER_RADIUS.sm,background:'var(--surface-hover)',color:'var(--text)',fontSize:FONT_SIZE.xs,outline:0,width:'100%',boxSizing:'border-box'}}/>
                </div>
                <button type="button" className="btn btn-primary" onClick={hm} disabled={sm_===sel.id} style={{height:36,padding:'4px 10px',fontSize:FONT_SIZE.xs,display:'flex',alignItems:'center',gap:4,whiteSpace:'nowrap'}}>{sm_===sel.id?<Loader size={13} className="animate-spin"/>:<Check size={13}/>} Save</button>
              </div>
              {me[sel.id]&&<div style={{color:'var(--danger)',fontSize:FONT_SIZE.xs}}>{me[sel.id]}</div>}
            </div>
          </div>
          <div style={{padding:`${SPACING.lg}px ${SPACING.xl}px`}}>
            <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',marginBottom:SPACING.sm}}>
              <div style={{color:'var(--text-secondary)',fontSize:FONT_SIZE.xs,fontWeight:600}}>API Keys</div>
              <span style={{fontSize:FONT_SIZE.xs,color:'var(--text-disabled)'}}>{sel.keys.filter(kk=>kk.isActive).length}/{sel.keys.length}</span>
            </div>
            {sel.keys.length===0 ? <div style={{padding:`${SPACING.lg}px 0`,textAlign:'center',color:'var(--text-disabled)',fontSize:FONT_SIZE.xs}}>No keys added yet</div>
            : <div style={{display:'flex',flexDirection:'column',gap:4}}>
              <div style={{display:'flex',alignItems:'center',gap:6,padding:'0 6px',fontSize:FONT_SIZE.micro,color:'var(--text-disabled)',textTransform:'uppercase',letterSpacing:0.4}}>
                <span style={{width:10}}/><span style={{width:70,flexShrink:0}}>Label</span><span style={{flex:1}}>Key</span><span style={{width:40,textAlign:'center'}}>St</span><span style={{width:80,textAlign:'center'}}>Tested</span><span style={{minWidth:75}}/>
              </div>
              {sel.keys.map(k => {
                const it = tid === k.id; const r = tr[k.id]; const st = r?.s??k.status;
                const tt = r?.t??k.lastTestedAt; const ee = r?.e??k.lastError;
                return (<div key={k.id} style={{display:'flex',alignItems:'center',gap:6,minHeight:34,padding:'3px 3px 3px 8px',border:'1px solid var(--border)',borderRadius:BORDER_RADIUS.sm,background:'var(--surface-hover)',flexWrap:'wrap'}}>
                  {k.isPrimary?<Star size={10} style={{color:'var(--warning)',flexShrink:0}}/>:<span style={{width:10,flexShrink:0}}/>}
                  <span style={{width:70,flexShrink:0,fontSize:FONT_SIZE.sm,color:'var(--text)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{k.label}</span>
                  <code style={{flex:1,fontSize:FONT_SIZE.xs,color:'var(--text-secondary)',fontFamily:'var(--font-mono)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{k.maskedKey}</code>
                  <span style={{width:40,display:'flex',alignItems:'center',justifyContent:'center',fontSize:FONT_SIZE.micro}}>{SM[st]??SM.untested} {st.slice(0,3)}</span>
                  <span style={{width:80,textAlign:'center',fontSize:FONT_SIZE.micro,color:'var(--text-disabled)'}}>{tt?new Date(tt).toLocaleDateString(locale==='zh'?'zh-CN':'en-US',{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'}):'—'}</span>
                  <div style={{display:'flex',gap:1}}>
                    <button type="button" onClick={()=>ht(k)} disabled={it} className="btn-ghost" style={{padding:4,minWidth:24,minHeight:24}} title="Test">{it?<Loader size={11} className="animate-spin"/>:<Wifi size={11}/>}</button>
                    {!k.isPrimary ? <button type="button" onClick={()=>onSetPrimaryKey(sel.id,k.id)} disabled={k.status!=='valid'} className="btn-ghost" style={{padding:4,minWidth:24,minHeight:24,opacity:k.status!=='valid'?0.35:1}} title={k.status!=='valid'?'Test first':'Set Primary'}><Star size={11}/></button>
                    : <span style={{fontSize:FONT_SIZE.micro,color:'var(--warning)',whiteSpace:'nowrap'}}>Current Primary</span>}
                    {!k.isPrimary && <button type="button" onClick={()=>sdt({p:sel.id,k:k.id})} className="btn-ghost" style={{padding:4,minWidth:24,minHeight:24,color:'var(--danger)'}} title="Delete"><Trash2 size={11}/></button>}
                  </div>
                  {ee&&<div style={{width:'100%',display:'flex',gap:4,alignItems:'center',color:'var(--danger)',fontSize:FONT_SIZE.xs,marginTop:1,paddingLeft:10}}><AlertCircle size={10}/>{ee}</div>}
                </div>);
              })}
            </div>}
            <div style={{display:'flex',gap:SPACING.sm,marginTop:SPACING.md,alignItems:'flex-end',borderTop:'1px solid var(--border)',paddingTop:SPACING.md}}>
              <div style={{flex:1,display:'flex',flexDirection:'column',gap:3}}>
                <input value={nl} onChange={e=>snl(e.target.value)} placeholder="Label" style={{height:36,padding:'0 10px',border:'1px solid var(--border)',borderRadius:BORDER_RADIUS.sm,background:'var(--surface-hover)',color:'var(--text)',fontSize:FONT_SIZE.xs,outline:0,width:'100%',boxSizing:'border-box'}}/>
                <input type="password" value={nk} onChange={e=>snk(e.target.value)} onKeyDown={e=>e.key==='Enter'&&hk()} placeholder="Paste new API key..." style={{height:36,padding:'0 10px',border:'1px solid var(--border)',borderRadius:BORDER_RADIUS.sm,background:'var(--surface-hover)',color:'var(--text)',fontSize:FONT_SIZE.xs,outline:0,width:'100%',boxSizing:'border-box'}}/>
              </div>
              <button type="button" className="btn" onClick={hk} disabled={!nk.trim()||ak===sel.id} style={{display:'flex',alignItems:'center',gap:6,whiteSpace:'nowrap'}}>{ak===sel.id?<Loader size={14} className="animate-spin"/>:<Plus size={14}/>} Add Key</button>
            </div>
            {ae&&<div style={{color:'var(--danger)',fontSize:FONT_SIZE.xs,marginTop:4}}>{ae}</div>}
          </div>
        </div>}
      </div>
      <ConfirmDialog open={dt!==null} title="Delete Key" message="Delete this API key? This action cannot be undone." confirmLabel="Delete" cancelLabel="Cancel" danger onConfirm={()=>{if(dt){onDeleteKey(dt.p,dt.k);sdt(null)}}} onCancel={()=>sdt(null)}/>
    </section>
  );
}
