export function renderPricingTab(container, data, t) {
  const form = document.createElement('form');
  form.className = 'model-price-form model-filter-bar model-filter-bar-price';
  form.dataset.role = 'usage-price-form';
  form.innerHTML = `<input name="providerId" maxlength="80" placeholder="${escapeText(t('modelProviderOptional', '供应商（可选）'))}"><input name="modelId" maxlength="160" required placeholder="${escapeText(t('modelID', '模型 ID'))}"><input name="inputPrice" type="number" min="0" step="0.000001" required placeholder="${escapeText(t('modelPriceInput', '输入 ($/1M)'))}"><input name="outputPrice" type="number" min="0" step="0.000001" required placeholder="${escapeText(t('modelPriceOutput', '输出 ($/1M)'))}"><input name="cacheReadPrice" type="number" min="0" step="0.000001" placeholder="${escapeText(t('modelPriceCacheRead', '缓存读取 ($/1M)'))}"><input name="cacheWritePrice" type="number" min="0" step="0.000001" placeholder="${escapeText(t('modelPriceCacheWrite', '缓存写入 ($/1M)'))}"><button class="primary" type="submit">${escapeText(t('modelSavePrice', '保存自定义费率'))}</button>`;
  container.append(form);
  const actionRow = document.createElement('div');
  actionRow.className = 'model-pricing-actions-row';
  actionRow.innerHTML = `<div class="model-pricing-meta"><span>${escapeText(t('modelPriceCatalogVer', '价格表版本'))}: <code>${escapeText(data?.catalogVersion || '-')}</code></span><span class="muted">· ${escapeText(t('modelPricedRequests', '已定价请求'))}: ${Number(data?.pricedRequestsCount) || 0} · ${escapeText(t('modelUnpricedRequests', '未定价请求'))}: ${Number(data?.unpricedRequestsCount) || 0}</span></div>
    <div class="model-actions-btns"><button type="button" class="primary" data-action="sync-pricing">${escapeText(t('modelSyncPricing', '从远端同步最新费率'))}</button></div>`;
  container.append(actionRow);
  const prices = data?.prices || [];
  if (!prices.length) return container.append(emptyState(t('modelNoPricingData', '暂无费率数据'), t('modelNoPricingHint', '同步远端费率后将在此显示。')));
  const table = document.createElement('div');
  table.className = 'model-table-wrapper';
  table.innerHTML = `<table class="model-prices-table"><thead><tr><th class="model-col-model">${escapeText(t('modelID', '模型 ID'))}</th><th class="model-col-num">${escapeText(t('modelPriceInput', '输入 ($/1M)'))}</th><th class="model-col-num">${escapeText(t('modelPriceOutput', '输出 ($/1M)'))}</th><th class="model-col-num">${escapeText(t('modelPriceCacheRead', '缓存读取 ($/1M)'))}</th><th class="model-col-num">${escapeText(t('modelPriceCacheWrite', '缓存写入 ($/1M)'))}</th><th class="model-col-source">${escapeText(t('modelPriceSource', '来源'))}</th><th class="model-col-actions">${escapeText(t('modelActions', '操作'))}</th></tr></thead><tbody>
    ${prices.map((price) => `<tr><td class="model-col-model"><strong title="${escapeText(price.modelId)}">${escapeText(price.modelId)}</strong>${price.providerId ? `<br><small class="muted" title="${escapeText(price.providerId)}">${escapeText(price.providerId)}</small>` : ''}</td>
      <td class="model-col-num">${money(price.inputPriceMicro)}</td><td class="model-col-num">${money(price.outputPriceMicro)}</td><td class="model-col-num">${money(price.cacheReadPriceMicro)}</td><td class="model-col-num">${money(price.cacheWritePriceMicro)}</td>
      <td class="model-col-source"><span class="model-price-source-tag">${escapeText(price.source || 'builtin')}</span></td><td class="model-col-actions">${price.source === 'manual' ? `<button type="button" class="danger btn-sm" data-action="delete-custom-price" data-provider="${escapeText(price.providerId || '')}" data-model="${escapeText(price.modelId)}">${escapeText(t('delete', '删除'))}</button>` : '<span class="muted">-</span>'}</td></tr>`).join('')}
    </tbody></table>`;
  container.append(table);
}

function money(micro) { return `$${((Number(micro) || 0) / 1_000_000).toFixed(4)}`; }
function emptyState(title, hint) { const node = document.createElement('div'); node.className = 'model-empty-state'; node.innerHTML = `<strong>${escapeText(title)}</strong><p class="muted">${escapeText(hint)}</p>`; return node; }
function escapeText(value) { return String(value ?? '').replace(/[&<>'"]/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]); }
