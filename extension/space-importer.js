/**
 * Tabliss Importer, Exporter, and Workspace Reset Controller (<180 lines).
 */

export function createSpaceImporter({
  t,
  nativeCall,
  broadcastRevision,
  updateSnapshot,
  onBackToOverview,
}) {
  function renderImportPreview(container, previewData, rawTablissJson, activeWorkspaceId, currentSnapshot) {
    container.replaceChildren();

    const header = document.createElement('div');
    header.className = 'inspector-heading';
    header.innerHTML = `
      <div style="display:flex;align-items:center;gap:8px;">
        <button class="inspector-back" type="button"><svg class="icon"><use href="#i-chevron-left" /></svg><span>${t('cancel', '取消')}</span></button>
        <h2>${t('importPreview', '导入预览')}</h2>
      </div>
    `;
    header.querySelector('.inspector-back').onclick = () => onBackToOverview();

    const body = document.createElement('div');
    body.className = 'inspector-body';

    const infoSec = document.createElement('div');
    infoSec.className = 'inspector-section';
    infoSec.innerHTML = `
      <div class="inspector-card">
        <p><strong>${t('importSummary', '配置概览')}</strong></p>
        <p>${t('background', '背景')}: <code>${previewData?.background_json?.key || 'background/colour'}</code></p>
        <p>${t('widgetsCount', '可导入组件数')}: <strong>${previewData?.widgets?.length || 0}</strong></p>
      </div>
      <div style="display:flex;gap:8px;margin-top:12px;">
        <button type="button" class="cancel-btn">${t('cancel', '取消')}</button>
        <button type="button" class="primary confirm-btn">${t('confirmImport', '确认导入并覆盖')}</button>
      </div>
    `;

    infoSec.querySelector('.cancel-btn').onclick = () => onBackToOverview();
    infoSec.querySelector('.confirm-btn').onclick = async () => {
      const confirmBtn = infoSec.querySelector('.confirm-btn');
      confirmBtn.disabled = true;
      try {
        const result = await nativeCall('workspace_save_from_tabliss', {
          workspaceId: activeWorkspaceId,
          tabliss: rawTablissJson,
          expectedRevision: currentSnapshot.revision,
        });
        updateSnapshot(result);
        broadcastRevision();
        onBackToOverview();
      } catch (err) {
        confirmBtn.disabled = false;
      }
    };

    body.append(infoSec);
    container.append(header, body);
  }

  async function startImportFile(container, activeWorkspaceId, currentSnapshot) {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json,application/json';
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const tablissJson = JSON.parse(text);
        const preview = await nativeCall('workspace_tabliss_preview', { tabliss: tablissJson });
        renderImportPreview(container, preview, tablissJson, activeWorkspaceId, currentSnapshot);
      } catch (err) {
        // Handled via nativeCall toast
      }
    };
    input.click();
  }

  async function exportTabliss(activeWorkspaceId, workspaceName = 'workspace') {
    try {
      const data = await nativeCall('workspace_export_tabliss', { workspaceId: activeWorkspaceId });
      const jsonStr = JSON.stringify(data, null, 2);
      const blob = new Blob([jsonStr], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      const safeName = String(workspaceName || 'workspace').replace(/[^a-zA-Z0-9_\u4e00-\u9fa5-]/g, '_');
      a.href = url;
      a.download = `natives-${safeName}-tabliss.json`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (err) {}
  }

  async function resetWorkspace(activeWorkspaceId, template, expectedRevision) {
    const result = await nativeCall('workspace_reset', {
      workspaceId: activeWorkspaceId,
      template,
      expectedRevision,
    });
    updateSnapshot(result);
    broadcastRevision();
  }

  return {
    startImportFile,
    exportTabliss,
    resetWorkspace,
  };
}
