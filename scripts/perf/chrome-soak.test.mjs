// Probe accounting must match Chrome: local disconnect does not emit onDisconnect.
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { installResourceProbe, frameSample } from './chrome-soak.mjs';
let disconnected = 0, remoteClose;
const port = {disconnect:()=>{disconnected++;},onDisconnect:{addListener:fn=>{remoteClose=fn;}}};
const sandbox = {setTimeout:()=>1,clearTimeout:()=>{},setInterval:()=>2,clearInterval:()=>{},chrome:{runtime:{connectNative:()=>port}}};
runInNewContext(`(${installResourceProbe.toString()})()`,sandbox);
const first = sandbox.chrome.runtime.connectNative('isolated');
assert.equal(sandbox.soakResources().nativePorts,1);
first.disconnect();
assert.equal(disconnected,1);
assert.equal(sandbox.soakResources().nativePorts,0);
sandbox.chrome.runtime.connectNative('isolated');remoteClose();
assert.equal(sandbox.soakResources().nativePorts,0);
sandbox.setTimeout(()=>{},10);sandbox.setInterval(()=>{},10);
assert.equal(sandbox.soakResources().timeouts,1);assert.equal(sandbox.soakResources().intervals,1);
sandbox.clearInterval(1);sandbox.clearTimeout(2);
assert.equal(sandbox.soakResources().timeouts,0);assert.equal(sandbox.soakResources().intervals,0);
console.log('Chrome soak probe: explicit/remote disconnect and shared timer cancellation passed');

const transient = {url:()=> 'http://isolated/',isDetached:()=>false,evaluate:async()=>{throw Error('Execution context was destroyed, most likely because of a navigation');}};
assert.equal((await frameSample(transient)).unavailable,'navigation');
assert.equal((await frameSample({...transient,isDetached:()=>true})).unavailable,'navigation');
await assert.rejects(frameSample({...transient,evaluate:async()=>{throw Error('unexpected probe failure');}}),/unexpected probe failure/);
assert.equal((await frameSample({...transient,evaluate:async()=>({visibility:'hidden',resources:{nativePorts:0}})})).resources.nativePorts,0);
console.log('Chrome soak frame sampling: navigation marked explicitly; unexpected failure still rejects');
assert.equal((await frameSample({...transient,evaluate:async()=>{throw Error('Frame was detached');}})).unavailable,'navigation');
