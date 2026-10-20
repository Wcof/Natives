import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('../../modules/fund/ui/src/otc-ops.js',import.meta.url),'utf8');
for (const operation of ['buy','sell','convert']) {
  let submit, sequence=0, lost=true;
  const writes=[];
  const box={querySelectorAll:()=>[],innerHTML:''};
  const root={querySelector:selector=>selector==='[data-orders]'?box:selector==='form'?{reset(){}}:{value:'before_1500'},querySelectorAll:()=>[]};
  const home={sensitive:()=>false,requestId:()=>`id-${++sequence}`,panel:(_title,_html,mount)=>mount(root),submit:(_root,fn)=>{submit=fn;},failure:()=>{}};
  const Fund={state:{language:'en'},t:key=>key,home,actions:{},api:async(method,_url,body)=>{
    if(method==='GET')return{orders:[],availableShares:'100.0000'};
    writes.push(body);if(lost){lost=false;throw Error('response lost after commit');}return{ok:true};
  }};
  vm.runInNewContext(source,{Fund,Date,Intl});
  Fund.otcOps[operation]({account:'a',fundCode:'110022'});
  const values={date:'2026-10-18',amount:'10.00',shares:'2.0000',target:'005827'};
  await submit(values);await new Promise(resolve=>setTimeout(resolve,3));await submit(values);
  assert.equal(writes[0].requestId,writes[1].requestId,`${operation} retry must reuse the committed request`);
  await submit({...values,amount:'20.00',shares:'3.0000'});
  assert.notEqual(writes[1].requestId,writes[2].requestId);
}
console.log('OTC buy/sell/convert unknown-result retry idempotency: PASS');

for (const [available, expected] of [
  ['0.0003', ['0.0000', '0.0001', '0.0001', '0.0003']],
  ['100.0003', ['25.0000', '33.3334', '50.0001', '100.0003']],
]) {
  const input = { value: '' };
  const buttons = ['1/4', '1/3', '1/2', 'all'].map(quick => ({ dataset: { quick } }));
  const box = { querySelectorAll: () => [], innerHTML: '' };
  const root = { querySelector: selector => selector === '[data-orders]' ? box : input,
    querySelectorAll: selector => selector === '[data-quick]' ? buttons : [] };
  const home = { sensitive: () => false, panel: (_title, _html, mount) => mount(root), submit() {}, failure(error) { throw error; } };
  const Fund = { state: { language: 'en' }, t: key => key, home, actions: {},
    api: async () => ({ orders: [], availableShares: available }) };
  vm.runInNewContext(source, { Fund, Date, Intl });
  Fund.otcOps.sell({ account: 'a', fundCode: '110022' });
  await new Promise(resolve => setImmediate(resolve));
  for (const [index, button] of buttons.entries()) {
    button.onclick();
    assert.equal(input.value, expected[index], `${available}: ${button.dataset.quick} must preserve four-place precision`);
  }
}
console.log('OTC fractional/full redemption preserves exact share units: PASS');
