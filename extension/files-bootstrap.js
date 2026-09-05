const filesParams = new URLSearchParams(location.search);
const loadFiles = () => import('./files.js').catch((error) => {
  const status = document.getElementById('status');
  if (status) { status.textContent = `页面初始化失败：${error?.message || '未知错误'}`; status.className = 'error'; }
  const retry = document.getElementById('retry');
  if (retry) { retry.hidden = false; retry.onclick = () => location.reload(); }
});

if (filesParams.has('ui-harness') || filesParams.has('self-test')) {
  const harness = document.createElement('script');
  harness.src = 'ui-harness.js';
  harness.onload = loadFiles;
  document.currentScript.before(harness);
} else {
  loadFiles();
}
