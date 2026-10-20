import assert from 'node:assert/strict';
import {appLifecycle} from './app-lifecycle.js';
let receive, released=0, disconnected=0, resolveRead;
const events=[];
const lifecycle=appLifecycle(event=>events.push(event),'fund',{
  subscribe:callback=>{receive=callback;return()=>released++;},
  client:{call:()=>new Promise(resolve=>{resolveRead=resolve;}),disconnect:()=>disconnected++},
});
const unrelated=receive({type:'apps_changed',appId:'tokenusage',maintenance:true});
// The unrelated update reads current authority without stopping this module.
resolveRead({app:{enabled:true}});
await unrelated;
assert.equal(events.length,0);
const pending=receive({type:'resync',reason:'reconnect'});
lifecycle.close();resolveRead({app:{enabled:false}});await pending;
assert.equal(events.length,0,'a late read cannot stop a released owner');
assert.equal(released,1);assert.equal(disconnected,1);
lifecycle.resume();
const recovery=receive({type:'resync',reason:'reconnect'});
resolveRead({app:{enabled:false}});await recovery;
assert.deepEqual(events,[{type:'maintenance',appId:'fund'}]);
await receive({type:'apps_changed',appId:'fund',maintenance:true});
assert.equal(events.length,2);
lifecycle.close();
console.log('Cross-entry maintenance, authority resync, stale response and BFCache disposer passed');
