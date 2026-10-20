import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

const document = new EventTarget(), window = new EventTarget(), timers = new Map();
document.hidden = false;
let now = 0, sequence = 0;
const context = vm.createContext({document, window, console, Date:{now:()=>now},
  setInterval(fn,ms){const id=++sequence;timers.set(id,{fn,ms});return id;},
  clearInterval(id){timers.delete(id);}, reportError(error){throw error;},
});
vm.runInContext(readFileSync(new URL('./widget-clock.js',import.meta.url),'utf8')
  .replaceAll('export function','function'),context);
const subscribe=context.subscribeWidgetClock;
const calls=Array(20).fill(0),dispose=[];
for(let i=0;i<20;i++){
  dispose.push(subscribe(()=>calls[i]++,i===19?10000:1000));
  assert.equal(timers.size,1);
}
now=1000;[...timers.values()][0].fn();
assert.deepEqual(calls,Array(19).fill(1).concat(0));
document.hidden=true;document.dispatchEvent(new Event('visibilitychange'));
assert.equal(timers.size,0);
now=15000;document.hidden=false;document.dispatchEvent(new Event('visibilitychange'));
assert.equal(timers.size,1);assert.equal(calls[19],1,'resume updates absolute time immediately');
const pagehide=new Event('pagehide');pagehide.persisted=true;window.dispatchEvent(pagehide);
assert.equal(timers.size,0);
now=30000;window.dispatchEvent(new Event('pageshow'));assert.equal(timers.size,1);
for(const stop of dispose)stop();
assert.equal(timers.size,0);
window.dispatchEvent(new Event('pageshow'));assert.equal(timers.size,0);
const slow=subscribe(()=>{},10000);assert.equal([...timers.values()][0].ms,10000);
slow();
console.log('20 clocks share one visible timer; hidden, BFCache, absolute-time resume and final disposer: PASS');
