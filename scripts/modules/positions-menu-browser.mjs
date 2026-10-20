// 真实 Chrome 渲染器、合成投影：菜单生命周期及边界，不作为账本验收证据。
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';

export async function verifyPositionsMenu(page) {
  const out = resolve(import.meta.dirname, '../../.tmp/positions-menu-ux');
  mkdirSync(out, { recursive: true });
  const requestsBefore = await page.evaluate(() => calls.length);
  await page.keyboard.press('Escape');
  for (const theme of ['archive', 'volt']) {
    for (const width of [320, 360, 640, 1280]) {
      await page.setViewportSize({ width, height: 600 });
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      const row = page.locator('.pos-more').first().locator('xpath=ancestor::article');
      const height = (await row.boundingBox()).height;
      await page.locator('.pos-more > summary').first().click();
      const menu = page.locator('.pos-action-menu:popover-open');
      await menu.waitFor({ state: 'visible' });
      assert.equal((await row.boundingBox()).height, height, 'popover must not increase row height');
      const bounds = await menu.boundingBox();
      assert.ok(bounds.x >= 7 && bounds.y >= 7 && bounds.x + bounds.width <= width - 7 && bounds.y + bounds.height <= 593, `menu fits ${theme} ${width}`);
      assert.equal(await menu.locator('[role=group]').count(), 3);
      assert.equal(await menu.locator('[role=menuitem]').count(), 10, 'all existing fund actions are retained');
      const name = await menu.locator('.pos-menu-name').boundingBox();
      assert.ok(name.y >= bounds.y && name.y + name.height <= bounds.y + bounds.height, 'opening identifies the current holding');
      if (width === 640) await page.screenshot({ path: resolve(out, `menu-${theme}-synthetic.png`) });
      assert.equal(await page.evaluate(() => document.activeElement.dataset.posAct), 'otcBuy', 'N09: otc rows lead with the OTC buy flow');
      await page.keyboard.press('End');
      assert.equal(await page.evaluate(() => document.activeElement.dataset.posAct), 'rules');
      await page.keyboard.press('ArrowDown');
      assert.equal(await page.evaluate(() => document.activeElement.dataset.posAct), 'otcBuy');
      await page.keyboard.press('Escape');
      assert.equal(await page.locator('.pos-more[open]').count(), 0);
      assert.equal(await page.evaluate(() => document.activeElement.tagName), 'SUMMARY');
      await page.locator('.pos-more > summary').first().press('ArrowUp');
      assert.equal(await page.evaluate(() => document.activeElement.dataset.posAct), 'rules');
      await page.keyboard.press('Tab');
      assert.equal(await page.locator('.pos-more[open]').count(), 0);
      await page.locator('.pos-more > summary').first().press('Space');
      await menu.waitFor({ state: 'visible' });
      await page.locator('#pos-display-toggle').focus();
      await page.mouse.click(2, 2);
      await page.waitForFunction(() => !document.querySelector('.pos-action-menu:popover-open'));
      assert.equal(await page.locator('.pos-more[open]').count(), 0);
    }
  }
  await page.setViewportSize({ width: 640, height: 320 });
  await page.evaluate(() => { document.body.style.zoom = '2'; });
  await page.locator('.pos-more > summary').first().click();
  const short = await page.locator('.pos-action-menu:popover-open').boundingBox();
  assert.ok(short.x >= 7 && short.y >= 7 && short.x + short.width <= 633 && short.y + short.height <= 313, '200% zoom fits short viewport');
  await page.keyboard.press('End');
  const last = await page.locator('.pos-action-menu:popover-open [data-pos-act=rules]').boundingBox();
  assert.ok(last.y >= short.y && last.y + last.height <= short.y + short.height, 'last item is inside zoomed scroll viewport');
  assert.equal(await page.locator('.pos-more[open]').count(), 1, 'internal menu scrolling keeps it open');
  await page.keyboard.press('Escape');
  await page.evaluate(() => { document.body.style.zoom = ''; });
  assert.equal(await page.evaluate(() => calls.length), requestsBefore, 'menu interactions do not add business requests');
  await page.setViewportSize({ width: 640, height: 800 });
  await page.locator('#pos-tab-all').click();
  await page.setViewportSize({width:640,height:320});
  await page.locator('.pos-more > summary').first().click();
  const scroll = await page.evaluate(() => {
    const el = document.querySelector('#view-positions'), before = el.scrollTop;
    el.scrollTop = before > 0 ? 0 : el.scrollHeight - el.clientHeight;
    return { before, after: el.scrollTop };
  });
  assert.notEqual(scroll.after, scroll.before, 'fixture exercises actual holdings scroll');
  await page.waitForFunction(() => !document.querySelector('.pos-action-menu:popover-open'));
  await page.locator('.pos-more > summary').first().click();
  await page.locator('.pos-more > summary').nth(1).focus();
  await page.keyboard.press('Enter');
  assert.equal(await page.locator('.pos-more[open]').count(), 1, 'only one row opens at once');
  assert.equal(await page.locator('.pos-more').first().getAttribute('open'), null);
  await page.locator('[data-pos-toggle]').first().click();
  assert.equal(await page.locator('.pos-action-menu:popover-open').count(), 0, 'collapsing account removes menu');
  await page.locator('[data-pos-toggle]').first().click();
  await page.locator('#pos-tab-otc').click();
  await page.locator('.pos-more > summary').first().click();
  await page.evaluate(() => { Fund.state.language = 'en'; document.dispatchEvent(new Event('fund:language-changed')); });
  assert.doesNotMatch(await page.locator('.pos-action-menu:popover-open').innerText(), /[\u4e00-\u9fff]/);
  assert.equal(await page.evaluate(() => document.activeElement.dataset.posAct), 'otcBuy');
  await page.evaluate(() => { Fund.state.language = 'zh-CN'; document.dispatchEvent(new Event('fund:language-changed')); });
  await page.locator('.pos-more > summary').first().click();
  console.log('positions menu: grouping / row height / outside / keyboard / single open / collapse / i18n / Archive+Volt / 320–1280px / 200% zoom passed');
}
