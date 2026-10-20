import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source=readFileSync(new URL('../../modules/fund/ui/src/core.js',import.meta.url),'utf8');
for(const [method,statuses,success,options] of [['GET',[409,200],true],['GET',[409,409],false],['POST',[409],false],['GET',[409],false,{retryConflict:false}]]) {
 let calls=0;
 const context={window:{parent:{postMessage(){}}},document:{querySelectorAll:()=>[]},fetch:async()=>{
  const status=statuses[calls++];assert.ok(status,'unexpected extra retry');return{status,ok:status===200,json:async()=>status===200?{value:1}:{error:'FIN_CONFLICT'}};
 },setTimeout,clearTimeout};
 vm.runInNewContext(source,context);
 const request=context.Fund.api(method,'/api/positions/detail',undefined,options);
 if(success)assert.equal((await request).value,1);else await assert.rejects(request,error=>error.status===409);
 assert.equal(calls,statuses.length);
}
console.log('snapshot reads retry one revision conflict; writes and repeated conflicts remain explicit: PASS');

const context={window:{parent:{postMessage(){}}},document:{querySelectorAll:()=>[]},setTimeout,clearTimeout};
vm.runInNewContext(source,context);
for(const language of ['zh-CN','en']){
 context.Fund.state.language=language;
 assert.equal(context.Fund.localizeApiError(409,'INFORMATION_REFRESH_IN_PROGRESS',''),'fundHomeInformationBusy');
 assert.equal(context.Fund.localizeApiError(409,'FIN_CONFLICT',''),'fundHomeDataConflict');
 const invalid=context.Fund.localizeApiError(400,'OTC_INVALID','invalid detail');
 assert.equal(invalid.includes('invalid detail'),language==='zh-CN');
 assert.equal(context.Fund.localizeApiError(500,'FIN_DB','private internal path').includes('private internal path'),false);
}
console.log('information task contention is distinct from financial conflicts; localized safe validation details: PASS');
