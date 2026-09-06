
export function createDiskUsage({ $, call, t, entryIcon, formatSize, parentAndName, currentPath }) {
  let diskUsagePath;

  async function show(path = currentPath()) {
    if (!path) return;
    const button = path === currentPath() ? $('disk-usage') : undefined;
    if (button) button.disabled = true;
    const dialog = $('usage-modal'); const body = $('usage-body'); const back = $('usage-back');
    diskUsagePath = path; if (!dialog.open) dialog.showModal(); body.replaceChildren(Object.assign(document.createElement('p'), { className: 'muted', textContent: t('loading', '加载中…') }));
    back.disabled = parentAndName(path).parent === path;
    try {
      const result = await call('disk_usage', { path }, crypto.randomUUID());
      if (diskUsagePath !== path) return;
      $('usage-title').textContent = `${t('diskUsage', '占用透视')} · ${formatSize(result.bytes)}`;
      body.replaceChildren(Object.assign(document.createElement('p'), { className: 'preview-meta muted', textContent: `${path} · ${result.files} ${t('filesCount', '个文件')}` }));
      const list = document.createElement('div'); list.className = 'usage-list';
      for (const item of (result.items || []).slice(0, 20)) {
        const row = document.createElement(item.isDir ? 'button' : 'div'); row.className = 'usage-entry';
        const label = document.createElement('span'); label.className = 'usage-entry-label'; label.append(entryIcon(item), document.createTextNode(` ${item.name}`));
        const meter = document.createElement('span'); meter.className = 'usage-entry-meter'; const fill = document.createElement('span'); fill.className = 'usage-entry-fill'; const ratio = result.bytes > 0 ? Math.min(100, Math.max(0, Number(item.size || 0) / result.bytes * 100)) : 0; fill.style.width = `${ratio.toFixed(2)}%`; meter.append(fill);
        const size = document.createElement('span'); size.className = 'usage-entry-size'; size.textContent = `${formatSize(item.size)} · ${ratio.toFixed(1)}%`; row.append(label, meter, size);
        if (item.isDir) { row.type = 'button'; row.onclick = () => show(`${path.replace(/\/$/, '')}/${item.name}`); }
        list.append(row);
      }
      body.append(list);
      if (result.errors || result.truncated) body.append(Object.assign(document.createElement('p'), { className: 'muted', textContent: `${result.errors ? `${result.errors} ${t('usageErrors', '个项目无法读取')}` : ''}${result.truncated ? ` · ${t('usageTruncated', '扫描已达上限')}` : ''}` }));
      back.disabled = parentAndName(path).parent === path;
    } catch (error) { if (diskUsagePath === path) { body.replaceChildren(Object.assign(document.createElement('p'), { className: 'error', textContent: error.message })); back.disabled = true; } }
    finally { if (button) button.disabled = false; }
  }

  $('usage-close').onclick = () => $('usage-modal').close();
  $('usage-back').onclick = () => { const parent = parentAndName(diskUsagePath || '').parent; if (diskUsagePath && parent !== diskUsagePath) show(parent); };

  return { show };
}
