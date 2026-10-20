(() => {
const trends = (Fund.trends ||= {});
const dialog = document.getElementById('scan-overlay');
function renderScanIdle() {
  const content = document.getElementById('scan-content');
  content.replaceChildren();
  const message = document.createElement('p');
  message.textContent = Fund.t('fundScanUnavailable');
  content.append(message);
}
function openScan() { renderScanIdle(); if (!dialog.open) dialog.showModal(); }
function closeScan() { if (dialog.open) dialog.close(); }
window.openScan = openScan;
window.closeScan = closeScan;
window.renderScanIdle = renderScanIdle;
document.getElementById('scan-btn').onclick = openScan;
trends.scan = { openScan, closeScan, renderScanIdle };
})();
