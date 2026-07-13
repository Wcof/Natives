import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { classifyProviderReadiness, connectionFingerprint, normalizeDiscoveredModels, selectDiscoveredModel, canTestDiscoveredModel, selectAssistantModel } from './provider-model-selection';

describe('classifyProviderReadiness',()=>{
  it('no_provider',()=>assert.equal(classifyProviderReadiness([]),'no_provider'));
  it('no_model',()=>assert.equal(classifyProviderReadiness([{id:'p1',provider_type:'o',display_name:'O',has_active_key:false,models:[]}]),'no_model'));
  it('ready',()=>assert.equal(classifyProviderReadiness([{id:'p1',provider_type:'o',display_name:'O',has_active_key:true,models:[{id:'gpt-4'}]}]),'ready'));
});
describe('selectAssistantModel',()=>{
  it('null empty',()=>assert.equal(selectAssistantModel([]),null));
  it('first ready',()=>{const r=selectAssistantModel([{id:'p1',provider_type:'o',display_name:'O',has_active_key:true,default_model:'gpt-4o',models:[{id:'gpt-4o'}]}]);assert.notEqual(r,null);assert.equal(r!.providerId,'p1');assert.equal(r!.modelId,'gpt-4o');});
});
describe('connectionFingerprint',()=>{it('same',()=>assert.equal(connectionFingerprint('https://a.com','sk-abc'),connectionFingerprint('https://a.com','sk-abc')));it('diff',()=>assert.notEqual(connectionFingerprint('https://a.com','sk-abc'),connectionFingerprint('https://a.com','sk-xyz')));});
describe('normalizeDiscoveredModels',()=>{it('filters',()=>{const r=normalizeDiscoveredModels([{id:'gpt-4'},{id:''},{id:'  '}]);assert.equal(r.length,1);assert.equal(r[0]!.id,'gpt-4');});});
describe('selectDiscoveredModel',()=>{it('first',()=>{const r=selectDiscoveredModel([{id:'gpt-4'},{id:'claude'}]);assert.notEqual(r,null);assert.equal(r!.id,'gpt-4');});it('null empty',()=>assert.equal(selectDiscoveredModel([]),null));});
describe('canTestDiscoveredModel',()=>{it('true',()=>assert.equal(canTestDiscoveredModel([{id:'gpt-4'}]),true));});
