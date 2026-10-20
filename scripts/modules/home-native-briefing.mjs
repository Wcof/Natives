// R3/T07 简报回合验收（拆分自 home-native-browser.test.mjs 以守架构规模门禁）。
// 通过真实 Native 链执行第四类 AI 任务：市场范围证据预览（E6 真实概览）、
// 发送、保存与报告管理中的生命周期。
export async function deleteAllReports(frame, page) {
    // 诊断与简报各存了一份报告：逐个删除直到空态。
    while (await frame.locator('dialog[open] [data-report-op=delete]').count() > 0) {
      const before = await frame.locator('dialog[open] article').count();
      await frame.locator('dialog[open] [data-report-op=delete]').first().click();
      await frame.locator('dialog[open] [data-yes]').click();
      for (let i = 0; i < 50; i++) {
        if (await frame.locator('dialog[open] article').count() < before) break;
        await page.waitForTimeout(300);
      }
    }
    await frame.locator('dialog[open]').getByText('No saved reports yet', { exact: false }).waitFor({ timeout: 15000 });
}

export function verifyBriefing(frame, page, report, assert) {
  return (async () => {
    // R3/T07（方案 05 AI 四类任务）：盘前／盘后简报——第四类任务走真实链，
    // 证据预览携带市场／时段／日期范围与 E6 真实概览，不含单项 scope。
    // 保存上一份报告经 with_write 推进了 data_revision：先刷新快照再预览，
    // 不绕过过期校验（409 是正确行为，不是测试目标）。
    const runtimeFrameForRefresh = page.frames().find(f => f.url().startsWith('http://127.0.0.1:'));
    assert.ok(runtimeFrameForRefresh, 'runtime frame must exist for snapshot refresh');
    await runtimeFrameForRefresh.evaluate(() => Fund.home.refresh());
    await frame.locator('dialog[open] [name=task]').selectOption('briefing');
    assert.equal(await frame.locator('dialog[open] [data-briefing]').evaluate(el => el.hidden), false, 'briefing fields appear for briefing task');
    await frame.locator('dialog[open] [name=briefingMarket]').selectOption('CN');
    await frame.locator('dialog[open] [name=briefingSession]').selectOption('post');
    await frame.locator('dialog[open] [type=submit]').click();
    await frame.locator('dialog[open] [data-preview] details > summary').click();
    await frame.locator('dialog[open] [data-preview]').getByText('market-session-briefing', { exact: false }).waitFor({ timeout: 20000 });
    const briefingPreview = await frame.locator('dialog[open] [data-preview]').textContent();
    assert.ok(briefingPreview.includes('"task": "briefing"'), 'preview task is briefing');
    assert.ok(briefingPreview.includes('"session": "post"'), 'preview carries the briefing session');
    assert.ok(briefingPreview.includes('"date"'), 'preview carries the briefing date');
    assert.ok(briefingPreview.includes('quoteCoverage'), 'preview carries the honest quote coverage');
    await frame.locator('dialog[open] [data-send]').click();
    await frame.locator('dialog[open] [data-result]').getByText('Synthetic full-chain explanation', { exact: false }).waitFor({ timeout: 35000 });
    await frame.locator('dialog[open] [data-save-report]').click();
    await frame.locator('dialog[open] [data-result]').getByText('报告已保存', { exact: false }).waitFor({ timeout: 15000 });
    await frame.locator('dialog[open] [data-result]').getByText('报告已保存', { exact: false }).waitFor({ timeout: 15000 });
    report.steps.push({ name: 'R3 briefing round trip: market-scoped evidence preview with E6 coverage through real Native chain' });
  })();
}
