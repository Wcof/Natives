import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createFilesSearch } from './files-search.js';

function fixture() {
  const pending=[],cancelled=[],rendered=[];
  const session={currentPath:'/old',pageOffset:0,selectedPaths:new Set(),showHidden:false};
  const control=createFilesSearch({
    $:()=>null,
    call:(method,params,id)=>method==='search'?new Promise((resolve,reject)=>pending.push({params,id,resolve,reject})):(cancelled.push(params.requestId),Promise.resolve({})),
    t:(_key,fallback)=>fallback,session,setStatus(){},sortEntries:rows=>rows,
    renderEntries:()=>rendered.push(session.entries),renderSelection(){},updatePager(){},
    getRootPaths:()=>['/one','/two'],loadDirectory(){},iconElement:()=>({}),
  });
  return {control,pending,cancelled,rendered,session};
}

test('leaving a directory invalidates a search even if its cancellation races completion',async()=>{
  const f=fixture(),job=f.control.search('old');
  f.session.currentPath='/new';f.control.cancelActiveSearches();
  f.pending[0].resolve({entries:[{path:'/old/stale'}]});await job;
  assert.deepEqual(f.rendered,[]);
});

test('a failed global search cancels its remaining roots without cancelling a newer search',async()=>{
  const f=fixture();f.control.setGlobalMode(true);const job=f.control.search('global');
  f.pending[0].reject(new Error('unavailable'));await job;
  assert.ok(f.cancelled.includes(f.pending[1].id));
  f.pending[1].resolve({entries:[]});assert.deepEqual(f.rendered,[]);
});

test('cancel releases the pending debounce instead of launching it after navigation',async()=>{
  const f=fixture();f.control.debounceSearch('old',10);f.control.cancelActiveSearches();
  await new Promise(resolve=>setTimeout(resolve,30));
  assert.equal(f.pending.length,0);
});

test('an old global-search finalizer cannot erase a newer local cancellation owner',async()=>{
  const before=globalThis.document;globalThis.document={createTextNode:value=>value};
  try {
    const f=fixture();f.control.setGlobalMode(true);const old=f.control.search('old');
    f.control.setGlobalMode(false);const current=f.control.search('new');
    f.pending[0].resolve({entries:[]});f.pending[1].resolve({entries:[]});await old;
    f.control.cancelActiveSearches();assert.ok(f.cancelled.includes(f.pending[2].id));
    f.pending[2].resolve({entries:[]});await current;assert.deepEqual(f.rendered,[]);
  }finally{globalThis.document=before;}
});
