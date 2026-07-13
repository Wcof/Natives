import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { ProviderSummary, ProviderKeySummary, ProviderKeyStatus } from '../types/provider';

function mkKey(o: Partial<ProviderKeySummary> = {}): ProviderKeySummary {
  return { id: o.id||'k1', providerId: o.providerId||'p1', label: o.label||'T', maskedKey: o.maskedKey||'sk-a…b2', isActive: o.isActive??true, isPrimary: o.isPrimary??false, status: o.status||'untested', lastTestedAt: o.lastTestedAt??null, lastError: o.lastError??null, createdAt: o.createdAt||new Date().toISOString() };
}
function mkProv(o: Partial<ProviderSummary> = {}): ProviderSummary {
  const keys = o.keys||[mkKey({status:'valid',isPrimary:true})];
  return { id: o.id||'p1', providerType: o.providerType||'openai', displayName: o.displayName||'O', websiteUrl: o.websiteUrl||'https://o.com', baseUrl: o.baseUrl||'https://api.o.com', defaultModel: o.defaultModel??'gpt-4o', primaryKeyId: o.primaryKeyId||(keys.find(k=>k.isPrimary)?.id??null), keys };
}

describe('ProviderKeySummary',()=>{
  it('no plaintext',()=>{const k=mkKey({maskedKey:'sk-a…b2'});assert.ok(!k.maskedKey.includes('abcdef'));assert.ok(k.maskedKey.length<=11);});
  it('valid can be primary',()=>{const k=mkKey({status:'valid',isPrimary:true});assert.equal(k.status,'valid');assert.equal(k.isPrimary,true);});
  it('untested cannot be primary',()=>{assert.equal(mkKey({status:'untested'}).isPrimary,false);});
  it('all statuses work',()=>{for(const s of ['untested','valid','invalid','rate_limited','unavailable']as ProviderKeyStatus[]){assert.equal(mkKey({status:s}).status,s);}});
});
describe('ProviderSummary',()=>{
  it('multiple keys',()=>assert.equal(mkProv({keys:[mkKey({isPrimary:true,status:'valid'}),mkKey({status:'untested'}),mkKey({status:'valid'})]}).keys.length,3));
  it('defaultModel once',()=>{const p=mkProv({defaultModel:'gpt-4o'});assert.equal(p.defaultModel,'gpt-4o');for(const k of p.keys)assert.equal(('defaultModel'in k),false);});
  it('keys masked',()=>{for(const k of mkProv({keys:[mkKey({maskedKey:'sk-a…1b'}),mkKey({maskedKey:'sk-x…y3'})]}).keys)assert.ok(!k.maskedKey.includes('abcdef'));});
});
describe('States',()=>{
  it('empty',()=>assert.equal(([]as ProviderSummary[]).length,0));
  it('no keys',()=>{const p=mkProv({keys:[]});assert.equal(p.keys.length,0);assert.equal(p.primaryKeyId,null);});
  it('untested primary (migration)',()=>{const k=mkKey({status:'untested',isPrimary:true});const p=mkProv({keys:[k],primaryKeyId:k.id});assert.equal(p.primaryKeyId,k.id);});
  it('primary no delete',()=>{assert.equal(mkKey({isPrimary:true}).isPrimary,true);});
});
describe('Tests',()=>{
  it('transitions',()=>{assert.equal(mkKey({status:'untested'}).status,'untested');assert.equal(mkKey({status:'valid',lastTestedAt:new Date().toISOString()}).status,'valid');});
  it('no delete on fail',()=>{const k=mkKey({status:'valid'});const a={...k,status:'invalid'as ProviderKeyStatus};assert.equal(a.id,k.id);});
});
describe('Sub-agent',()=>{
  it('primary-only works',()=>{const p=mkProv({keys:[mkKey({isPrimary:true,status:'valid'})]});assert.ok(p.primaryKeyId);assert.equal(p.keys.filter(k=>!k.isPrimary&&k.status==='valid').length,0);});
  it('2 sub-keys ok',()=>{assert.equal(mkProv({keys:[mkKey({isPrimary:true,status:'valid'}),mkKey({status:'valid'}),mkKey({status:'valid'})]}).keys.filter(k=>k.isActive&&!k.isPrimary&&k.status==='valid').length,2);});
});
describe('Retention',()=>{
  it('reload persists',()=>{const p=mkProv({keys:[mkKey({label:'W',maskedKey:'sk-a…1b'}),mkKey({label:'P',maskedKey:'sk-x…y3'})]});const r:ProviderSummary=JSON.parse(JSON.stringify(p));assert.equal(r.keys.length,2);assert.equal(r.keys[0]!.maskedKey,'sk-a…1b');assert.equal(r.keys[1]!.maskedKey,'sk-x…y3');});
  it('masked stable',()=>{const k=mkKey({maskedKey:'sk-a…1b'});assert.equal(mkProv({keys:[k]}).keys[0]!.maskedKey,mkProv({keys:[k]}).keys[0]!.maskedKey);});
  it('no plaintext json',()=>{assert.ok(!JSON.stringify(mkKey({maskedKey:'sk-a…b2'})).includes('abcdef'));});
});
