// Production Fund UI with isolated HTTP responses; no user data or supplier credentials.
import assert from 'node:assert/strict';
import { readFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { chromium } from 'playwright-core';
import { installHomeFixture } from './home-browser-fixture.mjs';

const root = resolve(import.meta.dirname, '../../modules/fund/ui/src');
const shots = resolve(import.meta.dirname, '../../.tmp/information-fulltext');
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless:true });
try {
  for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1280,320]) {
    const page=await browser.newPage({viewport:{width,height:850}});
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    const css=['theme','layout','panel','home','investment'].map(name=>readFileSync(resolve(name==='theme'?resolve(root,'../dist'):root,`styles/${name}.css`),'utf8')).join('\n');
    await page.setContent(`<style>${css}</style>${readFileSync(resolve(root,'partials/view-home.html'),'utf8')}`);
    await page.evaluate(installHomeFixture);
    for(const file of ['core.js','home-i18n.js','home-overview.js','home-opportunities.js','home.js','home-range.js','home-manage.js','home-information-detail.js','home-evidence.js'])await page.addScriptTag({path:resolve(root,file)});
    await page.evaluate(({theme,language})=>{
      document.documentElement.dataset.theme=theme;Fund.state.language=language;Fund.state.token='synthetic';Fund.home.applyDisplayMode('amount',false);
      window.infoFixture={failRead:true,failBody:false,holdRead:false,holdBody:false,reads:0,fetches:0,body:'',source:'em_finance',fetched:false,supported:true};
      const request=window.fetch;
      window.fetch=async(url,options)=>{
        const f=infoFixture;
        if(String(url).includes('/api/information/item?')){
          f.reads++;
          const item={id:1,title:'Public story',sourceId:f.source,summary:'Saved summary',url:'https://finance.eastmoney.com/a/202610201234567890.html',contentText:f.body,completeness:f.body?'full':f.fetched?'unavailable':'link_only',contentVersion:1,contentSupported:f.supported,contentFetchedAt:f.fetched?'now':null,relations:[]};
          if(f.holdRead)await new Promise(r=>{window.releaseInfoRead=r;});
          return f.failRead?new Response(JSON.stringify({error:'INFORMATION_DB'}),{status:500}):new Response(JSON.stringify(item));
        }
        if(String(url).endsWith('/api/information/content')){
          f.fetches++;if(f.holdBody)await new Promise(r=>{window.releaseInfoBody=r;});
          if(f.failBody)return new Response(JSON.stringify({error:'INFORMATION_UPSTREAM'}),{status:502});
          f.body='Full article body <script>never execute</script>';f.fetched=true;return new Response(JSON.stringify({content:{id:1}}));
        }
        return request(url,options);
      };
    },{theme,language});
    await page.evaluate(()=>{void Fund.home.openInformationDetail(1);});
    await page.locator('dialog[open] .home-form-error').filter({hasText:/\S/}).waitFor({timeout:3000});
    assert.equal(await page.locator('dialog[open] [data-info-retry]').count(),1,'read failures must open a visible retry dialog');
    await page.evaluate(()=>{infoFixture.failRead=false;infoFixture.failBody=true;});
    await page.locator('[data-info-retry]').focus();await page.keyboard.press('Enter');
    await page.locator('dialog[open] .home-form-error').filter({hasText:/\S/}).waitFor();
    assert.match(await page.locator('dialog[open]').innerText(),/Saved summary/);
    assert.equal(await page.locator('[data-info-fetch]').isEnabled(),true);
    await page.evaluate(()=>{infoFixture.failBody=false;});
    await page.locator('[data-info-fetch]').click();
    await page.locator('dialog[open] pre').filter({hasText:/Full article body/}).waitFor();
    assert.equal(await page.locator('dialog[open] script').count(),0);
    await page.evaluate(()=>{Fund.home.openExternal=async()=>{throw Error('EXTERNAL_LINK_BLOCKED');};});
    await page.locator('[data-info-url]').click();
    await page.locator('dialog[open] .home-form-error').filter({hasText:language==='en'?/Could not open the source/:/未能打开原文链接/}).waitFor();
    assert.match(await page.locator('dialog[open] pre').innerText(),/Full article body/);
    await page.evaluate(()=>{Fund.home.openExternal=async()=>({opened:true});});
    await page.locator('[data-info-url]').focus();await page.keyboard.press('Enter');
    assert.equal((await page.locator('dialog[open] .home-form-error').innerText()).trim(),'');
    const count=await page.evaluate(()=>infoFixture.fetches);
    await page.keyboard.press('Escape');
    await page.evaluate(()=>Fund.home.openInformationDetail(1));
    assert.equal(await page.evaluate(()=>infoFixture.fetches),count,'opening cached text does not refetch');
    assert.equal(await page.locator('dialog[open]').evaluate(el=>el.scrollWidth<=el.clientWidth+1),true);
    if(theme==='volt'&&language==='en'&&width===320)await page.screenshot({path:resolve(shots,'detail-narrow.png')});
    await page.keyboard.press('Escape');
    await page.evaluate(()=>{infoFixture.holdRead=true;void Fund.home.openInformationDetail(1);});
    await page.locator('dialog[open]').waitFor();await page.keyboard.press('Escape');
    await page.evaluate(()=>{releaseInfoRead();});
    await page.waitForTimeout(30);
    assert.equal(await page.locator('dialog[open]').count(),0,'late read must not reopen closed detail');
    await page.evaluate(()=>{infoFixture.holdRead=false;infoFixture.body='';infoFixture.fetched=false;infoFixture.holdBody=true;void Fund.home.openInformationDetail(1);});
    await page.waitForFunction(before=>infoFixture.fetches>before,count);
    await page.keyboard.press('Escape');
    await page.evaluate(()=>{releaseInfoBody();});
    await page.waitForTimeout(30);
    assert.equal(await page.locator('dialog[open]').count(),0,'late body must not reopen closed detail');
    await page.evaluate(()=>{infoFixture.holdBody=false;infoFixture.body='';infoFixture.fetched=true;infoFixture.supported=false;return Fund.home.openInformationDetail(1);});
    assert.equal(await page.locator('dialog[open] [data-info-fetch]').count(),0,'unsupported identities cannot request a body');
    assert.match(await page.locator('dialog[open]').innerText(),/Saved summary/);
    assert.match(await page.locator('dialog[open]').innerText(),language==='en'?/no readable text/:/暂未提供/);
    assert.deepEqual(errors,[]);
    await page.close();
  }
  console.log('Information detail Chrome checks passed: 8 theme/language/width combinations, read/body retry, cache, safe text, keyboard and late close.');
} finally {await browser.close();}
