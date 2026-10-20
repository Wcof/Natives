// Trusted owner form with a recording Native client; no real Keychain calls.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {chromium} from 'playwright-core';
const root=resolve(import.meta.dirname,'../..');
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try{
 const page=await browser.newPage();await page.setContent('<button id="focus">Focus</button><iframe></iframe>');
 await page.addScriptTag({content:readFileSync(resolve(root,'extension/module-secret-broker.js'),'utf8').replace('export function','window.createModuleSecretBroker = function')});
 await page.evaluate(()=>{
  window.calls=[];window.replies=[];window.frame=document.querySelector('iframe');frame.contentWindow.postMessage=m=>replies.push(m);
  window.session={frame,generation:'generation',challenge:'challenge',instanceId:'instance',capabilities:['platform.secret'],host:{call:async(method,params)=>{calls.push({method,params});}}};
  window.broker=createModuleSecretBroker({context:()=>session,t:k=>k});
  window.request=extra=>({source:frame.contentWindow,data:{type:'natives:secret',op:'edit',accountId:'channel:1:token',requestId:`test-${replies.length}`,generation:'generation',challenge:'challenge',...extra}});
  document.getElementById('focus').focus();
  void broker.handle(request({}));
 });
 await page.locator('dialog input').fill('ONLY-IN-TRUSTED-FORM');
 await page.locator('dialog [type=submit]').click();await page.locator('dialog').waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>calls[0].params.value),'ONLY-IN-TRUSTED-FORM');
 assert.equal(await page.evaluate(()=>calls[0].method),'app:secret');
 assert.equal(await page.evaluate(()=>JSON.stringify(replies).includes('ONLY-IN-TRUSTED-FORM')),false);
 assert.equal(await page.evaluate(()=>document.activeElement.id),'focus');
 await page.evaluate(()=>void broker.handle(request({value:'REJECT-MODULE-PLAINTEXT'})));
 assert.equal(await page.locator('dialog').count(),0);assert.equal(await page.evaluate(()=>calls.length),1);
 await page.evaluate(()=>void broker.handle(request({op:'delete'})));
 await page.locator('dialog').waitFor();await page.keyboard.press('Escape');
 await page.locator('dialog').waitFor({state:'detached'});assert.equal(await page.evaluate(()=>calls.length),1);
 await page.evaluate(()=>void broker.handle(request({op:'delete'})));
 await page.locator('dialog [type=submit]').click();await page.locator('dialog').waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>calls[1].params.confirm),true);
 await page.evaluate(()=>void broker.handle(request({})));
 await page.locator('dialog').waitFor();await page.evaluate(()=>broker.dispose());await page.locator('dialog').waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>calls.length),2);
 assert.equal(await page.evaluate(()=>replies.at(-1).result.cancelled),true);
 console.log('trusted secret Chrome: source form / no plaintext reply / injected field rejection / delete confirmation / cancellation / focus passed');
}finally{await browser.close();}
