// Real isolated source controls and public news detail paths.
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
export async function verifyInformationData(page,frame,report){
 const screenshots=resolve(import.meta.dirname,'../../.tmp/information-layout/native');mkdirSync(screenshots,{recursive:true});
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 // Public finance must work before any position has been created.
 await runtime.evaluate(async()=>{Fund.home.applyDisplayMode('amount',false);});
 await frame.locator('#tab-news').click();
 await frame.locator('#news-page-body .info-item').first().waitFor({timeout:30000});
 await frame.locator('#news-page-refresh').filter({hasText:/刷新|Refresh/}).waitFor();
 const publicData=await runtime.evaluate(()=>Fund.api('GET','/api/information/items?scope=market'));
 assert.ok(publicData.items.length>0,'public finance is available with no holdings');
 assert.ok(publicData.items.every(item=>item.sourceId==='em_finance'));
 const privateData=await runtime.evaluate(()=>Fund.api('GET','/api/information/items?scope=holdings'));
 assert.equal(privateData.items.length,0,'public stories do not imply a holding relation');
 report.finance={count:publicData.items.length,first:publicData.items[0].title,publishedAt:publicData.items[0].publishedAt};
 await frame.locator('#tab-home').click();
 await runtime.evaluate(async()=>{
  const asOf=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  await Fund.api('POST','/api/openings',{account:'Information fixture',instrumentKind:'stock',market:'SSE',fundCode:'600519',fundName:'Information fixture stock',quantity:'10',totalCost:'100',asOf});
  Fund.home.applyDisplayMode('amount',false);
  window.informationRefreshes=0;const request=fetch;
  window.fetch=(url,options)=>{if(String(url).includes('/api/information/refresh')&&options?.method==='POST')informationRefreshes++;return request(url,options);};
 });
 await frame.locator('#tab-news').click();
 await frame.locator('#news-page-refresh').click();
 await runtime.waitForFunction(()=>informationRefreshes===1);
 await frame.locator('#news-page-refresh').filter({hasText:/刷新|Refresh/}).waitFor();
 const data=await runtime.evaluate(()=>Fund.api('GET','/api/information/items?scope=holdings'));
 assert.ok(data.items.length>0,'real first-open collection must produce real public articles');
 assert.equal(await runtime.evaluate(()=>informationRefreshes),1);
 report.publicInformation={count:data.items.length,sources:data.sources,first:data.items[0].title,publishedAt:data.items[0].publishedAt};
 await runtime.evaluate(()=>Fund.actions.loadNewsPage());
 assert.equal(await runtime.evaluate(()=>informationRefreshes),1,'source attempt freshness prevents duplicate automatic collections');
 await frame.locator('#news-page-refresh').focus();await frame.locator('#news-page-refresh').press('Enter');
 await runtime.waitForFunction(()=>informationRefreshes===2);
 await frame.locator('#news-page-refresh').filter({hasText:/刷新|Refresh/}).waitFor();
 for(const theme of ['archive','volt'])for(const language of ['zh_CN','en'])for(const width of [1280,320]){
  await page.evaluate(({theme,language})=>chrome.storage.local.set({'natives-theme':theme,'natives-language':language}),{theme,language});
  await page.setViewportSize({width,height:900});
  await frame.locator('#news-page-body .info-item').first().waitFor();
  assert.ok(await runtime.evaluate(()=>document.querySelector('.main-column').scrollWidth<=document.querySelector('.main-column').clientWidth+1));
  await page.screenshot({path:resolve(screenshots,`${theme}-${language}-${width}.png`)});
 }
 assert.equal(await runtime.evaluate(()=>informationRefreshes),2,'theme/language changes do not collect again');
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
 assert.equal(await frame.locator('#news-page-refresh').isDisabled(),true);
 assert.equal(await frame.locator('#news-page-body .info-item').count(),0);
 report.steps.push({name:'real isolated first-open public news collection, fresh-cache reuse, keyboard refresh, eight UI combinations and privacy'});
}
export async function verifyNativeInformation(page,frame,report){
  // Strategy CRUD and automatic source policy have actual controls, not API-only coverage.
  await frame.locator('#home-sources-manage').click();
  await frame.locator('dialog[open] [data-thesis]').click();
  await frame.locator('dialog[open] [name=name]').fill('Acceptance thesis');
  await frame.locator('dialog[open] [name=holdingReason]').fill('Evidence, not automatic trading.');
  await frame.locator('dialog[open] [name=verifyMetrics]').fill('Review disclosures.');
  await frame.locator('dialog[open] [name=invalidCondition]').fill('Facts contradict rationale.');
  await frame.locator('dialog[open] [type=submit]').evaluate(node=>node.scrollIntoView({block:'center'}));
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-thesis-edit]').first().click();
  await frame.locator('dialog[open] [name=holdingReason]').fill('Updated rationale.');
  await frame.locator('dialog[open] [type=submit]').evaluate(node=>node.scrollIntoView({block:'center'}));
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-thesis-archive]').first().click();
  await frame.locator('dialog[open] [data-thesis-restore]').first().click();
  await frame.locator('dialog[open] [data-thesis-edit]').first().waitFor();
  for(const source of ['em_stock_ann','em_stock_news','em_fund_ann']){
    await frame.locator(`dialog[open] [name=auto-${source}]`).evaluate(node=>node.scrollIntoView({block:'center'}));
    await frame.locator(`dialog[open] [name=auto-${source}]`).uncheck();
    await frame.locator(`dialog[open] [name=auto-${source}]:not(:checked):not(:disabled)`).waitFor();
  }
  await frame.locator('dialog[open] [data-close]').click();
  await frame.locator('#home-info-refresh').click();
  await frame.locator('#home-info-refresh').filter({hasText:/Refresh|刷新/}).waitFor({timeout:30000});
  report.steps.push({name:'strategy create/edit/archive/restore; all sources disabled with explicit disabled refresh'});
  await frame.locator('#home-sources-manage').click();
  for(const source of ['em_stock_ann','em_stock_news','em_fund_ann']){
    await frame.locator(`dialog[open] [name=auto-${source}]`).evaluate(node=>node.scrollIntoView({block:'center'}));
    await frame.locator(`dialog[open] [name=auto-${source}]`).check();
    await frame.locator(`dialog[open] [name=auto-${source}]:checked:not(:disabled)`).waitFor();
  }
  await frame.locator('dialog[open] [data-close]').click();
  await frame.locator('#home-info-all').evaluate(node=>node.scrollIntoView({block:'center'}));
  await frame.locator('#home-info-all').click();
  await frame.locator('#news-page-body [name=collectionDays]').fill('30');
  await frame.locator('#news-page-body [data-info-filter] [type=submit]').evaluate(node=>node.scrollIntoView({block:'center'}));
  await frame.locator('#news-page-body [data-info-filter] [type=submit]').click();
  await frame.locator('#news-page-body [name=collectionDays][value="30"]').waitFor();
  await frame.locator('#tab-home').click();
  await frame.locator('#news-page-body').waitFor({state:'hidden'});
  await frame.locator('#home-info-refresh').click();
  await frame.locator('#home-info-refresh').filter({hasText:/Refresh|刷新/}).waitFor({timeout:30000});
  const fetched=await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(()=>Fund.api('GET','/api/information/items?scope=holdings'));
  report.information={count:fetched.items.length,sources:fetched.sources.map(s=>({id:s.sourceId,error:s.lastErrorCode,count:s.lastCount}))};
  if(fetched.items.length){
    await frame.locator('#home-info-all').evaluate(node=>node.scrollIntoView({block:'center'}));
    await frame.locator('#home-info-all').click();
    const announcement=fetched.items.find(i=>i.sourceId==='em_stock_ann');
    await frame.locator('#news-page-body [data-info-open'+(announcement?'="'+announcement.id+'"':'')+']').first().click();
    await frame.locator('dialog[open] [data-close]').waitFor();
    const fetchContent=frame.locator('dialog[open] [data-info-fetch]');
    if(await fetchContent.count()){
      await frame.locator('dialog[open]').getByText('Cached text is over 24 hours old',{exact:false}).waitFor();
      await fetchContent.click();
      await frame.locator('dialog[open] [data-info-fetch]').waitFor({state:'detached',timeout:30000});
      await frame.locator('dialog[open] pre').waitFor({timeout:30000});
      assert.ok((await frame.locator('dialog[open] pre').innerText()).length>20);
      const cached=await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(async()=>{
        const list=await Fund.api('GET','/api/information/items?scope=holdings');
        const item=list.items.find(i=>i.sourceId==='em_stock_ann'&&i.hasContent);
        const detail=await Fund.api('GET','/api/information/item?id='+item.id);
        if(detail.contentCacheExpired)throw Error('newly fetched body marked expired');
        const task=await Fund.api('POST','/api/information/content',{id:item.id,expectedVersion:detail.contentVersion,requestId:'body-cache-acceptance'});
        for(let n=0;n<100;n++){
          const status=await Fund.api('GET','/api/information/status?taskId='+task.taskId);
          if(status.result)return status.result;
          await new Promise(resolve=>setTimeout(resolve,50));
        }
        throw Error('cache task timed out');
      });
      assert.equal(cached.content.cached,true);
      report.steps.push({name:'real public announcement content fetched through owned task; fresh cache reused; completeness preserved'});
    }else{
      assert.equal(announcement,undefined);
      report.steps.push({name:'window has no announcement; news keeps source link without unsupported body-fetch action'});
    }
    await frame.locator('dialog[open] [data-close]').click();
    await frame.locator('#tab-home').click();
    const alertButton=frame.locator('[data-alert-defer]').first();
    if(await alertButton.count()){
      await alertButton.click();await frame.locator('dialog[open] [name=until]').waitFor();
      await frame.locator('dialog[open] [type=submit]').click();await frame.locator('dialog[open]').waitFor({state:'detached'});
      await frame.locator('#home-info-alerts').getByText('Deferred',{exact:false}).first().waitFor();
      report.steps.push({name:'information reminder deferred through real native date form'});
    }
    await frame.locator('#tab-news').click();
    await frame.locator('#news-page-body [data-info-filter] [name=sort]').selectOption('latest');
    await frame.locator('#news-page-body [data-info-filter] [name=type]').selectOption('repurchase');
    await frame.locator('#news-page-body [data-info-filter] [name=q]').fill('zz-no-matching-title');
    await frame.locator('#news-page-body [data-info-filter] [type=submit]').evaluate(node=>node.scrollIntoView({block:'center'}));
  await frame.locator('#news-page-body [data-info-filter] [type=submit]').click();
    await frame.locator('#news-page-body').getByText('No saved information in this scope;', {exact:false}).waitFor();
    report.steps.push({name:'real public information refresh/list/detail/keyword filter (coverage recorded, no invented source success)'});
  }else report.steps.push({name:'real public refresh returned no items; explicit source outcomes recorded; article interaction unverified'});

}
