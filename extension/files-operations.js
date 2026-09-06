
export function createFileOperations({ $, call, t, session, setStatus, toast, updateProgress, setOperationCancelable, openModal, loadDirectory, renderSelection, renderStatusBar, selectedItems, navigate, parentAndName, kindFromName, markSelfOpened, link }) {
  let activeImport;
  let activeBatch;
  let fileClipboard;
  let lastFailedOperation;

  function isBusy() { return Boolean(activeImport || activeBatch); }
  function getBatch() { return activeBatch; }
  function getClipboard() { return fileClipboard; }
  function activeUploadId() { return activeImport?.uploadId; }
  function cancelActive() { if (activeImport) activeImport.cancelled = true; if (activeBatch) activeBatch.cancelled = true; }

  function clearRetryOperation() { lastFailedOperation = undefined; $('retry-operation').hidden = true; }
  function rememberFailedOperation(operation) { lastFailedOperation = operation; $('retry-operation').hidden = false; }

  function bytesToBase64(bytes) { let binary = ''; for (let index = 0; index < bytes.length; index += 0x8000) binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000)); return btoa(binary); }
  function countRenamedPaths(sourcePaths = [], finalPaths = []) { return finalPaths.reduce((count, path, index) => count + (sourcePaths[index] && parentAndName(sourcePaths[index]).name !== parentAndName(path).name ? 1 : 0), 0); }
  function migrateTrackedPath(oldPath, newPath) { link.migrateEditorViewPath(oldPath, newPath); const migrate = (path) => path === oldPath || path.startsWith(`${oldPath}/`) ? `${newPath}${path.slice(oldPath.length)}` : path; link.migrateChangedPath(oldPath, newPath); }
  function migrateBatchPaths(sourcePaths = [], finalPaths = [], errors = [], skipped = []) { const blocked = new Set([...errors.map((error) => error.path), ...skipped]); let output = 0; for (const source of sourcePaths) { if (blocked.has(source)) continue; const target = finalPaths[output++]; if (target) migrateTrackedPath(source, target); } }

  async function retryFailedOperation() { const operation = lastFailedOperation; if (!operation || activeImport || activeBatch) return; clearRetryOperation(); if (operation.method === 'import') return importFileList(operation.files, operation.destination, operation.restore); if (operation.method === 'image-import') { const editor = $('preview-body').querySelector('.file-editor'); if (!editor || link.getEditorState()?.path !== operation.editorPath) return setStatus(t('previewUnavailable', '预览已切换，无法重试图片导入'), 'error'); for (const file of operation.files || []) if (!await importImageIntoEditor(file, editor)) break; return; } if (operation.method === 'image-copy') { const editor = $('preview-body').querySelector('.file-editor'); if (!editor || link.getEditorState()?.path !== operation.editorPath) return setStatus(t('previewUnavailable', '预览已切换，无法重试图片复制'), 'error'); await copyImageIntoEditor(operation.paths?.[0], editor); return; } if (operation.method === 'trash_batch') return retryTrashOperation(operation.paths); return runBatchOperation(operation.method.replace(/_batch$/, ''), operation.paths, operation.dest); }
  async function retryTrashOperation(paths = []) {
    if (!paths.length || activeImport || activeBatch) return;
    const requestId = crypto.randomUUID(); activeBatch = { cancelled: false, requestId }; setOperationCancelable(true); setStatus(`${t('processing', '处理中')} ${paths.length} 项`); updateProgress(0, paths.length);
    try { const result = await call('trash_batch', { paths }, requestId); const errors = result?.errors || []; if (errors.length) rememberFailedOperation({ method: 'trash_batch', paths: errors.map((error) => error.path).filter(Boolean) }); const summary = `${result?.count || 0}/${paths.length} ${t('trashed', '项已移到废纸篓，可恢复')}`; toast(result?.cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || result?.cancelled ? 'error' : 'success'); } catch (error) { rememberFailedOperation({ method: 'trash_batch', paths }); setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); }
    const failed = lastFailedOperation?.method === 'trash_batch' ? new Set(lastFailedOperation.paths || []) : new Set();
    if (failed.size) { await loadDirectory(session.currentPath); session.selectedPaths = new Set([...failed].filter((path) => session.entries.some((item) => item.path === path))); session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; renderSelection(); rememberFailedOperation({ method: 'trash_batch', paths: [...session.selectedPaths] }); if (!session.selectedPaths.size) clearRetryOperation(); }
    else clearRetryOperation();
  }
  async function runBatchOperation(method, paths, dest) { await runBatchOperationNow(method, paths, dest); }
  async function runBatchOperationNow(method, paths, dest) { if (!paths?.length || activeImport || activeBatch) return; const requestId = crypto.randomUUID(); const scrollTop = document.querySelector('.content')?.scrollTop || 0; activeBatch = { cancelled: false, requestId }; setOperationCancelable(true); setStatus(`${t('processing', '处理中')} ${paths.length} 项`); updateProgress(0, paths.length); try { const result = await call(`${method}_batch`, { paths, ...(dest ? { dest } : {}) }, requestId); const resultKey = method === 'copy' ? 'copied' : method === 'move' ? 'moved' : 'duplicated'; const completed = result?.[resultKey] || []; const errors = result?.errors || []; const skipped = result?.skipped || []; const failedPaths = errors.map((error) => error.path).filter(Boolean); if (errors.length) rememberFailedOperation({ method: `${method}_batch`, paths: failedPaths, dest }); if (session.currentPath) { await loadDirectory(session.currentPath); const candidates = failedPaths.length ? failedPaths : completed.filter((path) => parentAndName(path).parent === session.currentPath); session.selectedPaths = new Set(candidates.filter((path) => session.entries.some((item) => item.path === path))); session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; renderSelection(); const viewport = document.querySelector('.content'); if (viewport) viewport.scrollTop = scrollTop; } const renamed = countRenamedPaths(paths, completed); const skippedSummary = skipped.length ? ` · ${skipped.length} ${t('skipped', '项已跳过')}` : ''; const summary = `${completed.length}/${paths.length} ${t('completed', '项已完成')}${renamed ? ` · ${renamed} ${t('renamed', '项已自动重命名')}` : ''}${skippedSummary}`; toast(result?.cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || result?.cancelled ? 'error' : 'success'); } catch (error) { rememberFailedOperation({ method: `${method}_batch`, paths, dest }); setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); } }

  
  async function openItem(item) { if (link.hasDirtyEditor()) { let allowed = false; await link.guardDirty(() => { allowed = true; }); if (!allowed || link.hasDirtyEditor()) return; } try { if (item.isDir) return navigate(item.path); markSelfOpened(item.path); await call('open', { path: item.path }); toast(t('opened', '已打开')); } catch (error) { setStatus(error.message, 'error'); } }
  async function openItemFromDoubleClick(item) { if (item.isDir) return navigate(item.path); const kind = item.kind || kindFromName(item.name); if (!['text', 'image', 'video'].includes(kind)) return openItem(item); if (link.hasDirtyEditor()) { let allowed = false; await link.guardDirty(() => { allowed = true; }); if (!allowed || link.hasDirtyEditor()) return; } session.selectedPaths = new Set([item.path]); session.lastSelectedIndex = session.entries.findIndex((entry) => entry.path === item.path); renderSelection(); $('maximize-preview')?.click(); }
  async function revealPath(path) { try { await call('reveal', { path }); } catch (error) { setStatus(error.message, 'error'); } }
  async function revealSelected() { const item = selectedItems()[0]; if (item) await revealPath(item.path); }
  async function openEditorSelected() { const item = selectedItems()[0]; if (!item) return; try { await call('editor', { path: item.path }); toast(t('openedEditor', '已在编辑器打开')); } catch (error) { setStatus(error.message, 'error'); } }
  async function copyPathSelectedFor(path) {
    if (!path) return;
    const text = String(path);
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
      else {
        const input = document.createElement('textarea');
        input.value = text; input.setAttribute('readonly', ''); input.style.position = 'fixed'; input.style.opacity = '0';
        document.body.append(input); input.select();
        if (!document.execCommand('copy')) throw new Error(t('clipboardUnavailable', '剪贴板不可用'));
        input.remove();
      }
      toast(t('copiedPath', '路径已复制'));
    } catch (error) { setStatus(error.message || t('clipboardUnavailable', '剪贴板不可用'), 'error'); }
  }
  async function copyPathSelected() {
    const paths = selectedItems().map((item) => item.path);
    if (!paths.length) return;
    const text = paths.join('\n');
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
      else {
        const input = document.createElement('textarea');
        input.value = text; input.setAttribute('readonly', ''); input.style.position = 'fixed'; input.style.opacity = '0';
        document.body.append(input); input.select();
        if (!document.execCommand('copy')) throw new Error(t('clipboardUnavailable', '剪贴板不可用'));
        input.remove();
      }
      toast(t('copiedPath', '路径已复制'));
    } catch (error) { setStatus(error.message || t('clipboardUnavailable', '剪贴板不可用'), 'error'); }
  }
  async function copyFileSelected() { const paths = selectedItems().filter((item) => !item.isDir).map((item) => item.path); if (!paths.length) return; try { await call('copy_paths', { paths }); toast(t('fileCopied', '文件已复制')); } catch (error) { setStatus(error.message, 'error'); } }

  
  async function createEntry(kind) { if (!session.currentPath) return; openModal({ title: t(kind === 'directory' ? 'newFolder' : 'newFile', kind === 'directory' ? '新建文件夹' : '新建文件'), label: t('name', '名称'), submit: async (name) => { try { await call(kind === 'directory' ? 'create_folder' : 'write_file', kind === 'directory' ? { parent: session.currentPath, name } : { parent: session.currentPath, name, data: '' }); link.setPendingSelectionPath(`${session.currentPath.replace(/[\\/]+$/, '')}/${name}`); await loadDirectory(session.currentPath); renderSelection(); toast(t('created', '已创建')); } catch (error) { setStatus(error.message, 'error'); } } }); }
  async function importDroppedImageUrls(value, destination = session.currentPath) {
    const urls = String(value || '').split(/[\r\n]+/).map((line) => line.trim()).filter((line) => line && !line.startsWith('#') && /^(https?:|data:image\/)/i.test(line)).slice(0, 5);
    const files = [];
    for (const url of urls) {
      try {
        const response = await fetch(url); if (!response.ok) continue;
        const blob = await response.blob(); if (!/^image\//i.test(blob.type)) continue;
        let name = ''; try { name = decodeURIComponent(new URL(url, location.href).pathname.split('/').pop() || ''); } catch {}
        if (!name || !/\.[a-z0-9]+$/i.test(name)) name = `image-${Date.now()}-${files.length + 1}.${(blob.type.split('/')[1] || 'png').replace('jpeg', 'jpg')}`;
        files.push(new File([blob], name, { type: blob.type }));
      } catch {}
    }
    if (files.length) await importFileList(files, destination);
  }
  async function importFileList(fileList, destination = session.currentPath, restore) {
    if (!destination) return;
    if (activeImport || activeBatch) return toast(t('processing', '处理中'));
    const files = [...fileList].slice(0, 200);
    const importSelection = new Set(restore?.selection || session.selectedPaths); const importScrollTop = Number.isFinite(restore?.scrollTop) ? restore.scrollTop : (document.querySelector('.content')?.scrollTop || 0);
    activeImport = { cancelled: false, totalBytes: files.reduce((total, file) => total + file.size, 0), completedBytes: 0 };
    setOperationCancelable(true);
    updateProgress(0, activeImport.totalBytes);
    const importTarget = `${t('importDestination', '导入到')} ${destination}`;
    setStatus(`${importTarget} · ${files.length} ${t('items', '个项目')}`);
    let importedCount = 0;
    const importedPaths = [];
    let failedCount = 0;
    let renamedCount = 0;
    const failedFiles = [];
    const directoryCache = new Map([[destination, true]]);
    async function ensureDirectory(relative) {
      let parent = destination;
      for (const segment of relative.split('/').filter(Boolean).slice(0, 32)) {
        if (activeImport.cancelled) throw new Error(t('importCancelled', '导入已取消'));
        if (!/^[^./\\\0]{1,255}$/.test(segment)) throw new Error(t('invalidImportPath', '导入路径无效'));
        const next = `${parent}/${segment}`;
        if (!directoryCache.has(next)) {
          try { await call('create_folder', { parent, name: segment }); }
          catch (error) { const stat = await call('stat', { path: next }); if (!stat?.found || !stat.isDir) throw error; }
          directoryCache.set(next, true);
        }
        parent = next;
      }
      return parent;
    }
    for (const file of files) {
      if (activeImport.cancelled) break;
      const uploadId = crypto.randomUUID();
      activeImport.uploadId = uploadId;
      try {
        const relative = String(file.webkitRelativePath || '').split('/');
        const parent = await ensureDirectory(relative.slice(0, -1).join('/'));
        const name = relative.at(-1) || file.name;
        await call('import_probe', { parent, name });
        const begun = await call('import_begin', { uploadId, parent, name, size: file.size, conflict: 'rename' });
        if (begun?.renamed) renamedCount++;
        const chunkSize = 512 * 1024;
        for (let offset = 0; offset < file.size; offset += chunkSize) {
          if (activeImport.cancelled) throw new Error(t('importCancelled', '导入已取消'));
          const bytes = new Uint8Array(await file.slice(offset, Math.min(offset + chunkSize, file.size)).arrayBuffer());
          await call('import_chunk', { uploadId, offset, data: bytesToBase64(bytes) });
          const transferred = activeImport.completedBytes + Math.min(offset + bytes.length, file.size); updateProgress(transferred, activeImport.totalBytes); setStatus(`${importTarget} · ${t('importing', '导入中')} ${file.name} · ${Math.min(offset + bytes.length, file.size)}/${file.size}`);
        }
        const result = await call('import_end', { uploadId });
        if (result?.path) importedPaths.push(result.path);
        importedCount++; activeImport.completedBytes += file.size; updateProgress(activeImport.completedBytes, activeImport.totalBytes);
      } catch (error) {
        await call('import_cancel', { uploadId }).catch(() => {});
        if (activeImport.cancelled) break;
        failedCount++;
        failedFiles.push(file);
        toast(`${file.name}：${error.message}`, 'error');
      }
    }
    const cancelled = activeImport.cancelled;
    activeImport = undefined;
    setOperationCancelable(false);
    updateProgress(0, 1, false);
    if (destination === session.currentPath) { const lastPath = importedPaths.at(-1); if (lastPath && parentAndName(lastPath).parent !== session.currentPath) { link.setPendingSelectionPath(lastPath); navigate(parentAndName(lastPath).parent); } else { await loadDirectory(session.currentPath); if (lastPath) { session.selectedPaths = new Set([lastPath]); session.lastSelectedIndex = session.entries.findIndex((item) => item.path === lastPath); renderSelection(); document.querySelector(`[data-path="${CSS.escape(lastPath)}"]`)?.scrollIntoView({ block: 'nearest' }); } else { session.selectedPaths = new Set([...importSelection].filter((path) => session.entries.some((item) => item.path === path))); session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; renderSelection(); const viewport = document.querySelector('.content'); if (viewport) viewport.scrollTop = importScrollTop; } } }
    const summary = `${importTarget} · ${importedCount}/${files.length} ${t('imported', '个文件已导入')}${renamedCount ? ` · ${renamedCount} ${t('renamed', '项已自动重命名')}` : ''}${failedCount ? ` · ${failedCount} ${t('failed', '项失败')}` : ''}`;
    if (failedFiles.length && !cancelled) rememberFailedOperation({ method: 'import', files: failedFiles, destination, restore: { selection: [...importSelection], scrollTop: importScrollTop } });
    toast(cancelled ? `${summary} · ${t('importCancelled', '导入已取消')}` : summary, cancelled || failedCount ? 'error' : 'success');
  }
  async function importImageIntoEditor(file, editor) {
    const editorState = link.getEditorState();
    if (!editorState || !/^image\//i.test(file.type || '') || !/\.(md|markdown|mdx)$/i.test(editorState.path) || activeImport || activeBatch) return false;
    const editorPath = editorState.path; const parent = parentAndName(editorPath).parent; const uploadId = crypto.randomUUID(); activeImport = { cancelled: false, uploadId, totalBytes: file.size, completedBytes: 0 }; setOperationCancelable(true); updateProgress(0, file.size);
    try {
      await call('import_probe', { parent, name: file.name }); await call('import_begin', { uploadId, parent, name: file.name, size: file.size, conflict: 'rename' });
      const chunkSize = 512 * 1024; for (let offset = 0; offset < file.size; offset += chunkSize) { if (activeImport.cancelled) throw new Error(t('importCancelled', '导入已取消')); const bytes = new Uint8Array(await file.slice(offset, Math.min(offset + chunkSize, file.size)).arrayBuffer()); await call('import_chunk', { uploadId, offset, data: bytesToBase64(bytes) }); updateProgress(offset + bytes.length, file.size); }
      const result = await call('import_end', { uploadId }); const name = result?.path ? parentAndName(result.path).name : file.name; const encoded = encodeURIComponent(name); const value = `![${name}](${encoded})`; const start = editor.selectionStart; editor.setRangeText(value, start, editor.selectionEnd, 'end'); editor.dispatchEvent(new Event('input', { bubbles: true })); toast(t('imageInserted', '图片已插入'), 'success'); await loadDirectory(session.currentPath); return true;
    } catch (error) { await call('import_cancel', { uploadId }).catch(() => {}); if (!activeImport?.cancelled) { rememberFailedOperation({ method: 'image-import', files: [file], editorPath }); toast(`${file.name}：${error.message}`, 'error'); } return false; }
    finally { activeImport = undefined; setOperationCancelable(false); updateProgress(0, 1, false); }
  }
  async function copyImageIntoEditor(path, editor) {
    const editorState = link.getEditorState();
    if (!editorState || !path || !editor || !/^\/(?!\/)/.test(path) || !/\.(md|markdown|mdx)$/i.test(editorState.path) || activeImport || activeBatch) return false;
    const editorPath = editorState.path; const parent = parentAndName(editorPath).parent;
    const requestId = crypto.randomUUID(); activeBatch = { cancelled: false, requestId }; setOperationCancelable(true); setStatus(t('processing', '处理中')); updateProgress(0, 1);
    try {
      const result = await call('copy_batch', { paths: [path], dest: parent }, requestId);
      if (activeBatch?.cancelled || result?.cancelled) throw new Error(t('operationCancelled', '操作已取消'));
      const copied = result?.copied?.[0]; if (!copied) throw new Error(t('imagePreviewUnavailable', '图片复制失败'));
      const name = parentAndName(copied).name; const value = `![${name}](${encodeURIComponent(name)})`; const start = editor.selectionStart; editor.setRangeText(value, start, editor.selectionEnd, 'end'); editor.dispatchEvent(new Event('input', { bubbles: true })); toast(t('imageInserted', '图片已插入'), 'success'); await loadDirectory(session.currentPath);
    } catch (error) { rememberFailedOperation({ method: 'image-copy', paths: [path], editorPath }); setStatus(error.message, 'error'); return false; }
    finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); }
    return true;
  }

  
  async function renameSelected() { const item = selectedItems()[0]; if (!item || session.selectedPaths.size > 1) return toast(t('singleSelectionRequired', '请先只选择一个项目')); openModal({ title: t('rename', '重命名'), label: t('newName', '新名称'), value: item.name, submit: async (name) => { if (name === item.name) return; try { const result = await call('rename', { path: item.path, name }); if (result?.path) migrateTrackedPath(item.path, result.path); session.selectedPaths.clear(); await loadDirectory(session.currentPath); const finalName = result?.path ? parentAndName(result.path).name : name; toast(`${t('renamed', '已重命名')}${finalName !== name ? ` · ${finalName}` : ''}`); } catch (error) { setStatus(error.message, 'error'); } } }); }
  async function transferNow(method) { const items = selectedItems(); if (!items.length || activeImport || activeBatch) return; openModal({ title: t(method === 'copy' ? 'copy' : 'move', method === 'copy' ? '复制到…' : '移动到…'), message: t('destinationHint', '请输入目标文件夹的完整路径'), label: t('destination', '目标文件夹路径'), submit: async (destination) => { const requestId = crypto.randomUUID(); const scrollTop = document.querySelector('.content')?.scrollTop || 0; activeBatch = { cancelled: false, requestId }; clearRetryOperation(); setOperationCancelable(true); setStatus(`${t('processing', '处理中')} ${items.length} 项`); updateProgress(0, 1); try { const result = await call(`${method}_batch`, { paths: items.map((item) => item.path), dest: destination }, requestId); const completed = result?.[method === 'copy' ? 'copied' : 'moved'] || []; if (method === 'move' && completed.length === items.length && completed.every((path) => typeof path === 'string')) items.forEach((item, index) => migrateTrackedPath(item.path, completed[index])); const skipped = result?.skipped || []; const errors = result?.errors || []; const failedPaths = [...errors, ...skipped].map((error) => error.path).filter(Boolean); if (errors.length) rememberFailedOperation({ method: `${method}_batch`, paths: errors.map((error) => error.path).filter(Boolean), dest: destination }); await loadDirectory(session.currentPath); const candidates = failedPaths.length ? failedPaths : result?.cancelled ? items.map((item) => item.path) : completed.filter((path) => parentAndName(path).parent === session.currentPath); session.selectedPaths = new Set(candidates.filter((path) => session.entries.some((item) => item.path === path))); session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; renderSelection(); const viewport = document.querySelector('.content'); if (viewport) viewport.scrollTop = scrollTop; const renamed = countRenamedPaths(items.map((item) => item.path), completed); const summary = `${completed.length}/${items.length} ${t('completed', '项已完成')}${renamed ? ` · ${renamed} ${t('renamed', '项已自动重命名')}` : ''}${skipped.length ? ` · ${skipped.length} ${t('skipped', '项已跳过')}` : ''}`; if (errors.length) setStatus(`${summary}：${errors.slice(0, 3).map((error) => `${error.path?.split('/').pop() || error.path}：${error.error}`).join('；')}`, 'error'); toast(result?.cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || result?.cancelled ? 'error' : 'success'); } catch (error) { rememberFailedOperation({ method: `${method}_batch`, paths: items.map((item) => item.path), dest: destination }); setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); } } }); }
  async function duplicateSelectedNow() { const items = selectedItems(); if (!items.length) return; if (activeImport || activeBatch) return toast(t('processing', '处理中')); const requestId = crypto.randomUUID(); const scrollTop = document.querySelector('.content')?.scrollTop || 0; activeBatch = { cancelled: false, requestId }; clearRetryOperation(); setOperationCancelable(true); setStatus(`${t('processing', '处理中')} ${items.length} 项`); updateProgress(0, items.length); try { const result = await call('duplicate_batch', { paths: items.map((item) => item.path) }, requestId); const completed = result?.duplicated || []; const errors = result?.errors || []; const failedPaths = errors.map((error) => error.path).filter(Boolean); const cancelled = Boolean(result?.cancelled || activeBatch?.cancelled); if (errors.length) rememberFailedOperation({ method: 'duplicate_batch', paths: failedPaths }); await loadDirectory(session.currentPath); const candidates = failedPaths.length ? failedPaths : cancelled ? items.map((item) => item.path) : completed; session.selectedPaths = new Set(candidates.filter((path) => session.entries.some((item) => item.path === path))); session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; renderSelection(); const viewport = document.querySelector('.content'); if (viewport) viewport.scrollTop = scrollTop; const summary = `${completed.length}/${items.length} ${t('duplicated', '项副本已创建')}`; if (errors.length) setStatus(`${summary}：${errors.slice(0, 3).map((error) => `${error.path?.split('/').pop() || error.path}：${error.error}`).join('；')}`, 'error'); toast(cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || cancelled ? 'error' : 'success'); } catch (error) { rememberFailedOperation({ method: 'duplicate_batch', paths: items.map((item) => item.path) }); setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); } }
  async function moveDroppedPaths(raw, destination) {
    let paths;
    try { paths = JSON.parse(raw); } catch { return; }
    if (!Array.isArray(paths) || !paths.length || !destination || activeImport || activeBatch) return;
    const requestId = crypto.randomUUID();
    activeBatch = { cancelled: false, requestId };
    clearRetryOperation();
    setOperationCancelable(true);
    setStatus(`${t('processing', '处理中')} ${paths.length} 项`);
    updateProgress(0, paths.length);
    try {
      const result = await call('move_batch', { paths, dest: destination }, requestId);
      const moved = result?.moved || [];
      const errors = result?.errors || [];
      const skipped = result?.skipped || [];
      migrateBatchPaths(paths, moved, errors);
      if (errors.length) rememberFailedOperation({ method: 'move_batch', paths: errors.map((error) => error.path).filter(Boolean), dest: destination });
      if (destination === session.currentPath) { link.setPendingSelectionPath(moved.at(-1)); await loadDirectory(session.currentPath); renderSelection(); }
      const renamed = countRenamedPaths(paths, moved); const summary = `${result?.moved?.length || 0}/${paths.length} ${t('moved', '已移动')}${renamed ? ` · ${renamed} ${t('renamed', '项已自动重命名')}` : ''}${skipped.length ? ` · ${skipped.length} ${t('skipped', '项已跳过')}` : ''}`;
      if (errors.length) setStatus(`${summary}：${errors.slice(0, 3).map((error) => `${error.path?.split('/').pop() || error.path}：${error.error}`).join('；')}`, 'error');
      toast(result?.cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || result?.cancelled ? 'error' : 'success');
    } catch (error) { rememberFailedOperation({ method: 'move_batch', paths, dest: destination }); setStatus(error.message, 'error'); }
    finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); }
  }
  async function copyDroppedUris(raw, destination) {
    const values = String(raw || '').split(/\r?\n/).map((value) => value.trim()).filter((value) => value && !value.startsWith('#'));
    const paths = [...new Set(values.flatMap((value) => { try { const url = new URL(value); if (url.protocol !== 'file:' || (url.hostname && url.hostname !== 'localhost')) return []; let path = decodeURIComponent(url.pathname); if (navigator.platform.startsWith('Win') && /^\/[A-Za-z]:\//.test(path)) path = path.slice(1).replaceAll('/', '\\'); return path.includes('\0') ? [] : [path]; } catch { return []; } }))];
    const imageUrls = values.filter((value) => /^(https?:|data:image\/)/i.test(value));
    if (imageUrls.length) await importDroppedImageUrls(imageUrls.join('\n'), destination);
    if (!paths.length || !destination || activeImport || activeBatch) return;
    const requestId = crypto.randomUUID(); activeBatch = { cancelled: false, requestId }; clearRetryOperation(); setOperationCancelable(true); updateProgress(0, paths.length); setStatus(`${t('processing', '处理中')} ${paths.length} 项`);
    try { const result = await call('copy_batch', { paths, dest: destination }, requestId); const errors = result?.errors || []; const skipped = result?.skipped || []; if (errors.length) rememberFailedOperation({ method: 'copy_batch', paths: errors.map((error) => error.path).filter(Boolean), dest: destination }); const copied = result?.copied || []; const renamed = countRenamedPaths(paths, copied); if (destination === session.currentPath) { link.setPendingSelectionPath(copied.at(-1)); await loadDirectory(session.currentPath); renderSelection(); } const summary = `${copied.length}/${paths.length} ${t('imported', '已导入')}${renamed ? ` · ${renamed} ${t('renamed', '项已自动重命名')}` : ''}${skipped.length ? ` · ${skipped.length} ${t('skipped', '项已跳过')}` : ''}`; if (errors.length) setStatus(`${summary}：${errors.slice(0, 3).map((error) => error.error).join('；')}`, 'error'); toast(result?.cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || result?.cancelled ? 'error' : 'success'); } catch (error) { rememberFailedOperation({ method: 'copy_batch', paths, dest: destination }); setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); }
  }

  
  function setClipboard(mode) { const paths = [...session.selectedPaths]; if (!paths.length) return; fileClipboard = { mode, paths }; renderStatusBar(); toast(`${paths.length} ${t(mode === 'copy' ? 'clipboardCopied' : 'clipboardCut', mode === 'copy' ? '项已复制' : '项已剪切')}`); }
  function clearFileClipboard() { fileClipboard = undefined; renderStatusBar(); toast(t('clipboardCleared', '剪贴板已清除')); }
  async function pasteClipboardNow(destination = session.currentPath) { if (!fileClipboard || !destination || activeImport || activeBatch) return; const clip = fileClipboard; const requestId = crypto.randomUUID(); activeBatch = { cancelled: false, requestId }; setOperationCancelable(true); setStatus(`${t('processing', '处理中')} ${clip.paths.length} 项`); updateProgress(0, clip.paths.length); try { const result = await call(`${clip.mode}_batch`, { paths: clip.paths, dest: destination }, requestId); const completed = result?.[clip.mode === 'copy' ? 'copied' : 'moved'] || []; if (clip.mode === 'move' && completed.length === clip.paths.length && completed.every((path) => typeof path === 'string')) clip.paths.forEach((path, index) => migrateTrackedPath(path, completed[index])); const skipped = result?.skipped || []; const errors = result?.errors || []; if (clip.mode === 'move' && !errors.length && !result?.cancelled) fileClipboard = undefined; if (destination === session.currentPath) await loadDirectory(session.currentPath); renderStatusBar(); const renamed = countRenamedPaths(clip.paths, completed); const summary = `${completed.length}/${clip.paths.length} ${t('pasted', '项已粘贴')}${renamed ? ` · ${renamed} ${t('renamed', '项已自动重命名')}` : ''}${skipped.length ? ` · ${skipped.length} ${t('skipped', '项已跳过')}` : ''}`; if (errors.length) setStatus(`${summary}：${errors.slice(0, 3).map((error) => `${error.path?.split('/').pop() || error.path}：${error.error}`).join('；')}`, 'error'); toast(result?.cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || result?.cancelled ? 'error' : 'success'); } catch (error) { rememberFailedOperation({ method: `${clip.mode}_batch`, paths: clip.paths, dest: destination }); setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); renderStatusBar(); } }

  
  async function trashSelected() {
    const items = selectedItems(); if (!items.length || activeImport || activeBatch) return;
    const run = async () => { const requestId = crypto.randomUUID(); activeBatch = { cancelled: false, requestId }; setOperationCancelable(true); setStatus(`${t('processing', '处理中')} ${items.length} 项`); updateProgress(0, 1); try { const result = await call('trash_batch', { paths: items.map((item) => item.path) }, requestId); const errors = result?.errors || []; session.selectedPaths.clear(); await loadDirectory(session.currentPath); const summary = `${result?.count || 0}/${items.length} ${t('trashed', '项已移到废纸篓，可恢复')}`; if (errors.length) setStatus(`${summary}：${errors.slice(0, 3).map((error) => `${error.path?.split('/').pop() || error.path}：${error.error}`).join('；')}`, 'error'); toast(result?.cancelled ? `${summary} · ${t('operationCancelled', '操作已取消')}` : summary, errors.length || result?.cancelled ? 'error' : 'success'); } catch (error) { rememberFailedOperation({ method: 'trash_batch', paths: items.map((item) => item.path) }); setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); } };
    if (items.some((item) => item.isDir)) openModal({ title: t('moveToTrash', '移到废纸篓'), message: `${t('trashFolderConfirm', '将所选文件夹移到系统废纸篓？可恢复。')} (${items.length})`, submit: run }); else await run();
  }

  
  async function extractArchive(item) { if (!item?.path || activeImport || activeBatch) return; const suggested = `${item.path.replace(/\.(zip|jar|tar|tgz|tbz2?|txz|gz|zst)$/i, '')}-extracted`; openModal({ title: t('extractArchive', '解压缩包'), message: t('extractArchiveHint', '请输入解压目标文件夹路径（支持 ZIP/TAR）'), label: t('destination', '目标文件夹路径'), value: suggested, submit: async (destination) => { try { const result = await call('extract_archive', { path: item.path, dest: destination }, crypto.randomUUID()); if (destination === session.currentPath) await loadDirectory(session.currentPath); toast(`${result?.files || 0} ${t('archiveExtracted', '个文件已解压')}`, 'success'); } catch (error) { setStatus(error.message, 'error'); } } }); }
  async function createZip() { const items = selectedItems(); if (!items.length || activeImport || activeBatch) return; openModal({ title: t('createArchive', '创建 ZIP'), message: t('createArchiveHint', '选中项目必须位于同一目录'), label: t('archiveName', '归档名称'), value: 'archive.zip', submit: async (name) => { const requestId = crypto.randomUUID(); activeBatch = { cancelled: false, requestId }; setOperationCancelable(true); setStatus(`${t('processing', '处理中')} ${items.length} 项`); updateProgress(0, 1); try { const result = await call('create_zip', { paths: items.map((item) => item.path), dest: session.currentPath, name }, requestId); if (activeBatch?.cancelled || result?.cancelled) throw new Error(t('operationCancelled', '操作已取消')); link.setPendingSelectionPath(result?.path); await loadDirectory(session.currentPath); renderSelection(); toast(`${result?.files || 0} ${t('archiveCreated', '个文件已打包')}`, 'success'); } catch (error) { setStatus(error.message, 'error'); } finally { activeBatch = undefined; setOperationCancelable(false); updateProgress(0, 1, false); } } }); }

  async function duplicateSelected() { await duplicateSelectedNow(); }
  async function transfer(method) { await transferNow(method); }
  async function pasteClipboard(destination = session.currentPath) { await pasteClipboardNow(destination); }

  return { isBusy, getBatch, getClipboard, activeUploadId, cancelActive, clearRetryOperation, rememberFailedOperation, retryFailedOperation, openItem, openItemFromDoubleClick, revealPath, revealSelected, openEditorSelected, copyPathSelected, copyPathSelectedFor, copyFileSelected, createEntry, importFileList, importDroppedImageUrls, importImageIntoEditor, copyImageIntoEditor, moveDroppedPaths, copyDroppedUris, renameSelected, transfer, duplicateSelected, pasteClipboard, setClipboard, clearFileClipboard, trashSelected, extractArchive, createZip, migrateTrackedPath, migrateBatchPaths, countRenamedPaths };
}
