// Exact native disclosure regression; no real account or provider data.
import assert from 'node:assert/strict';
export async function verifyFilterFocus(page,frame,report){
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 await runtime.waitForFunction(()=>Fund.home.snapshot()!=null);
 await runtime.evaluate(async()=>{
  const date=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  await Fund.api('POST','/api/openings',{account:'Filter fixture',instrumentKind:'stock',market:'SSE',fundCode:'600519',fundName:'Filter stock',quantity:'10',totalCost:'100',asOf:date});
  Fund.home.applyDisplayMode('amount',false);await Fund.home.refresh();
 });
 await frame.locator('.home-extra-research > summary').click();
 await frame.locator('.opportunity-filter-more > summary').click();
 await frame.locator('.home-extra-research > summary').click();
 for(const theme of ['archive','volt'])for(const language of ['zh_CN','en'])for(const width of [320,640,1280]){
  await page.evaluate(({theme,language})=>chrome.storage.local.set({'natives-theme':theme,'natives-language':language}),{theme,language});
  await page.setViewportSize({width,height:900});
  for(const view of ['home','positions','assets','tx']){
   const button=frame.locator('#tab-'+view);if(!await button.isVisible())await frame.locator('#btn-tools-menu').click();await button.click();if(view==='home')await frame.locator('.home-history > summary').click();await page.screenshot({animations:'disabled'});
  }
 }
 await page.setViewportSize({width:640,height:900});await runtime.evaluate(()=>document.body.style.zoom='2');
 for(const view of ['home','positions','assets','tx']){const button=frame.locator('#tab-'+view);if(!await button.isVisible())await frame.locator('#btn-tools-menu').click();await button.click();}
 await runtime.evaluate(()=>document.body.style.zoom='');
 await page.setViewportSize({width:1280,height:900});
 await frame.locator('#tab-assets').click();await frame.locator('#offline-add').click();
 await frame.locator('dialog[open] [name=name]').fill('Unsaved disclosure fixture');
 await page.evaluate(()=>chrome.storage.local.set({'natives-language':'zh_CN'}));await page.keyboard.press('Escape');
 await page.evaluate(()=>chrome.storage.local.set({'natives-language':'en'}));
 await frame.locator('#tab-home').click();await runtime.evaluate(()=>Fund.home.refresh());
 for(const selector of ['.home-extra-research','.opportunity-filter-more']){
  if(!await frame.locator(selector).evaluate(node=>node.open)){await frame.locator(selector+' > summary').focus();await frame.locator(selector+' > summary').press('Enter');}
  assert.equal(await frame.locator(selector).evaluate(node=>node.open),true);
 }
 await frame.locator('[data-opportunity-filters] [name=market]').selectOption('CN',{timeout:5000});
 assert.equal(await frame.locator('[data-opportunity-filters] [name=market]').inputValue(),'CN');
 report.steps.push({name:'Exact Native home advanced filter disclosure, locale/draft and keyboard-compatible selection'});
}
