import { UsageImporterWizard } from './model-usage-importer.js';

export async function handleUsageAction(controller, action, target) {
  if (action === 'select-usage-tab') {
    controller.view.setUsageTab(target.dataset.tab);
    await reload(controller);
  } else if (action === 'filter-usage-range') {
    controller.usageFilter.range = target.dataset.range;
    controller.usageFilter.page = 1;
    await reload(controller);
  } else if (action === 'filter-usage-dimension') {
    controller.usageFilter[target.dataset.filter] = target.value;
    controller.usageFilter.page = 1;
    await reload(controller);
  } else if (action === 'show-custom-range') {
    controller.usageFilter.range = 'custom';
    controller.render();
  } else if (action === 'prev-events-page') {
    if (controller.usageFilter.page > 1) controller.usageFilter.page--;
    await reload(controller);
  } else if (action === 'next-events-page') {
    controller.usageFilter.page++;
    await reload(controller);
  } else if (action === 'sync-pricing') {
    await busy(controller, async () => {
      controller.pricingData = await controller.api.syncUsagePrice();
      controller.view.showNotice(controller.t('modelSyncPricingSuccess', '价格表已成功从远端同步更新'));
    });
  } else if (action === 'delete-custom-price') {
    await busy(controller, async () => {
      controller.pricingData = await controller.api.deleteUsagePrice({ providerId: target.dataset.provider, modelId: target.dataset.model });
    });
  } else if (action === 'pick-import-file') {
    const input = controller.view.dialog.querySelector('[data-role="importer-file-input"]');
    if (input) {
      input.onchange = (event) => importFile(controller, event.target.files[0]);
      input.click();
    }
  } else return false;
  return true;
}

export async function handleUsageSubmit(controller, form) {
  if (form.dataset.role === 'usage-price-form') {
    const data = Object.fromEntries(new FormData(form));
    const micro = (value) => Math.round(Number(value || 0) * 1_000_000);
    await busy(controller, async () => {
      controller.pricingData = await controller.api.upsertUsagePrice({
        providerId: data.providerId.trim(), modelId: data.modelId.trim(),
        inputPriceMicro: micro(data.inputPrice), outputPriceMicro: micro(data.outputPrice),
        cacheReadPriceMicro: micro(data.cacheReadPrice), cacheWritePriceMicro: micro(data.cacheWritePrice),
      });
      form.reset();
    });
    return true;
  }
  if (form.dataset.role !== 'usage-custom-range-form') return false;
  const data = Object.fromEntries(new FormData(form));
  const start = new Date(data.startTime);
  const end = new Date(data.endTime);
  if (!Number.isFinite(start.getTime()) || !Number.isFinite(end.getTime()) || start >= end) {
    controller.showError(controller.t('modelInvalidTimeRange', '结束时间必须晚于开始时间'));
    return true;
  }
  controller.usageFilter = { ...controller.usageFilter, range: 'custom', startTime: start.toISOString(), endTime: end.toISOString(), page: 1 };
  await reload(controller);
  return true;
}

async function reload(controller) {
  await busy(controller, () => controller.loadUsageData());
}

async function busy(controller, work) {
  controller.view.setLoading(true);
  try {
    await work();
  } catch (error) {
    controller.showError(error);
  } finally {
    controller.view.setLoading(false);
    controller.render();
  }
}

async function importFile(controller, file) {
  if (!file) return;
  const progress = controller.view.dialog.querySelector('[data-role="importer-progress"]');
  const fill = controller.view.dialog.querySelector('[data-role="progress-fill"]');
  const previewNode = controller.view.dialog.querySelector('[data-role="importer-preview"]');
  if (progress) progress.hidden = false;
  if (previewNode) previewNode.hidden = true;
  controller.importerWizard = new UsageImporterWizard({ api: controller.api, t: controller.t });
  try {
    const preview = await controller.importerWizard.processFile(file, ({ percent }) => { if (fill) fill.style.width = `${percent}%`; });
    if (progress) progress.hidden = true;
    if (!previewNode) return;
    previewNode.hidden = false;
    previewNode.innerHTML = `<div class="model-importer-preview-card"><h4>${escapeText(controller.t('modelImportPreview', '导入数据概览'))}</h4>
      <p>${escapeText(controller.t('modelTotalRecords', '总记录数'))}: ${Number(preview.totalRecords) || 0}</p>
      <p>${escapeText(controller.t('modelTimeSpan', '时间跨度'))}: ${escapeText(preview.earliestRecord || '-')} ~ ${escapeText(preview.latestRecord || '-')}</p>
      <div class="model-form-actions"><button type="button" class="primary" data-role="confirm-import">${escapeText(controller.t('modelConfirmImport', '确认合并导入'))}</button><button type="button" data-role="cancel-import">${escapeText(controller.t('cancel', '取消'))}</button></div></div>`;
    previewNode.querySelector('[data-role="confirm-import"]').onclick = () => busy(controller, async () => {
      const result = await controller.importerWizard.commit();
      controller.view.showNotice(controller.t('modelImportSuccess', '成功导入 $1 条记录').replace('$1', result.importedCount));
      await controller.loadUsageData();
    });
    previewNode.querySelector('[data-role="cancel-import"]').onclick = () => { controller.importerWizard.cancel(); previewNode.hidden = true; };
  } catch (error) {
    if (progress) progress.hidden = true;
    controller.showError(error);
  }
}

function escapeText(value) {
  return String(value ?? '').replace(/[&<>'"]/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]);
}
