/**
 * Preview atomic capability (预览): owns the right-pane preview renderers and
 * the in-place text editor (state, autosave, conflict handling, image editor).
 * All page dependencies are injected once via createPreviewControllers(); the
 * `link` object is wired by the composition root after both capability modules
 * exist, breaking the preview ↔ operations dependency cycle lazily.
 */
export function createPreviewControllers({ $, call, t, session, setStatus, toast, formatSize, parentAndName, pathParts, entryIcon, iconElement, iconAction, isTextItem, TEXT_KINDS, isHostConnected, selectedItems, renderSelection, loadDirectory, remember, revealPath, link }) {
  let editorState;
  const editorViewStates = new Map();
  let imageEditorState;
  let editorRefreshToken = 0;
  let saveQueue = Promise.resolve();

  function getEditorState() { return editorState; }
  function getImageEditorState() { return imageEditorState; }
  function hasDirtyEditor() { return Boolean(editorState?.dirty || imageEditorState?.dirty); }

  function rememberEditorViewState() { const editor = $('preview-body')?.querySelector('.file-editor'); if (!editorState?.path || !editor) return; editorViewStates.set(editorState.path, { scrollTop: editor.scrollTop, selectionStart: editor.selectionStart, selectionEnd: editor.selectionEnd }); while (editorViewStates.size > 20) editorViewStates.delete(editorViewStates.keys().next().value); }
  function migrateEditorViewPath(oldPath, newPath) { const migrate = (path) => path === oldPath || path.startsWith(`${oldPath}/`) ? `${newPath}${path.slice(oldPath.length)}` : path; const moved = []; for (const [path, state] of editorViewStates) if (path === oldPath || path.startsWith(`${oldPath}/`)) moved.push([migrate(path), state]); for (const [path] of editorViewStates) if (path === oldPath || path.startsWith(`${oldPath}/`)) editorViewStates.delete(path); moved.forEach(([path, state]) => editorViewStates.set(path, state)); }
  function encodeBase64(value) { const bytes = new TextEncoder().encode(value); let binary = ''; for (let index = 0; index < bytes.length; index += 0x8000) binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000)); return btoa(binary); }

  function clearPreviewBody() { $('preview-body').replaceChildren(); }
  function previewButton(label, action) { const button = document.createElement('button'); button.textContent = label; button.onclick = action; return button; }
  function appendCopyPathAction(actions, item) {
    if (!actions.querySelector('[data-action="openEditor"]')) {
      const editor = previewButton(t('openEditor', '在编辑器打开'), () => call('editor', { path: item.path }).catch((error) => setStatus(error.message, 'error')));
      editor.dataset.action = 'openEditor'; actions.append(editor);
    }
    const file = previewButton(t('copyFile', '复制文件'), async () => { try { await call('copy_paths', { paths: [item.path] }); toast(t('fileCopied', '文件已复制')); } catch (error) { setStatus(error.message, 'error'); } });
    file.dataset.action = 'copyFile'; actions.append(file);
    const button = previewButton(t('copyPath', '复制路径'), () => link.copyPathSelectedFor(item.path)); button.dataset.action = 'copyPath'; actions.append(button);
  }
  function appendEditorPreviewActions(item) {
    const body = $('preview-body');
    if (!body || body.querySelector('[data-editor-preview-actions]')) return;
    const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.dataset.editorPreviewActions = 'true';
    const editor = previewButton(t('openEditor', '在编辑器打开'), () => call('editor', { path: item.path }).catch((error) => setStatus(error.message, 'error'))); editor.dataset.action = 'openEditor'; actions.append(editor);
    appendCopyPathAction(actions, item); body.append(actions);
  }
  function previewMeta(item) { const meta = document.createElement('p'); meta.className = 'preview-meta muted'; const fields = [[t('name', '名称'), item.name], [t('size', '大小'), formatSize(Number(item.size) || 0)], [t('createdAt', '创建'), Number(item.btime) ? new Date(item.btime).toLocaleString() : ''], [t('modifiedAt', '修改'), Number(item.mtime) ? new Date(item.mtime).toLocaleString() : ''], [t('path', '路径'), item.path]]; for (const [label, value] of fields) { if (!value) continue; const field = document.createElement('span'); field.className = 'preview-meta-field'; const key = document.createElement('strong'); key.textContent = `${label}:`; const text = document.createElement('span'); text.textContent = String(value); field.append(key, text); meta.append(field); } return meta; }
  function unsupportedPreviewMessage(item) {
    if (item.kind === 'image') return t('imagePreviewUnavailable', '图片预览暂不可用，请使用系统应用打开');
    if (item.kind === 'pdf') return t('pdfPreviewUnavailable', 'PDF 使用系统默认应用打开');
    if (item.kind === 'audio' || item.kind === 'video') return t('mediaPreviewUnavailable', '音视频使用系统默认应用打开');
    if (Number(item.size) > 2 * 1024 ** 2) return t('largeFilePreviewUnavailable', '大文件不会读入浏览器，请使用系统应用打开');
    return t('previewUnsupported', '此文件类型不在安全内置预览范围内。');
  }
  function renderUnsupportedPreview(item) { const body = $('preview-body'); body.append(previewMeta(item)); const note = document.createElement('p'); note.className = 'muted'; note.textContent = unsupportedPreviewMessage(item); body.append(note); const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.append(previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path))); appendCopyPathAction(actions, item); body.append(actions); }
  function renderPreviewError(item, message) { const body = $('preview-body'); const failed = document.createElement('p'); failed.className = 'error'; failed.textContent = message; body.append(failed); const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.append(previewButton(t('retry', '重试'), () => renderPreviewSelection()), previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path))); appendCopyPathAction(actions, item); body.append(actions); }

  async function openImageEditor(item) {
    const body = $('preview-body'); body.replaceChildren(previewMeta(item)); const loading = document.createElement('p'); loading.className = 'muted'; loading.textContent = t('loading', '加载中…'); body.append(loading);
    try {
      const result = await call('image_preview', { path: item.path }, crypto.randomUUID());
      if (!/^image\/(png|jpeg|gif|webp|bmp|avif)$/i.test(result?.mimeType || '') || typeof result.data !== 'string') throw new Error(t('imagePreviewUnavailable', '图片预览暂不可用，请使用系统应用打开'));
      const image = new Image(); image.src = `data:${result.mimeType};base64,${result.data}`; await image.decode();
      if (image.naturalWidth * image.naturalHeight > 60e6) throw new Error(t('imagePreviewUnavailable', '图片预览暂不可用，请使用系统应用打开'));
      loading.remove(); const canvas = document.createElement('canvas'); canvas.className = 'image-editor-canvas'; canvas.tabIndex = 0; canvas.setAttribute('aria-label', t('editorLabel', '文件编辑器')); canvas.setAttribute('aria-keyshortcuts', 'Escape Meta+Z Control+Z Meta+S Control+S'); const canvasWrap = document.createElement('div'); canvasWrap.className = 'image-editor-wrap'; canvasWrap.append(canvas); const cropPreview = document.createElement('div'); cropPreview.className = 'image-crop-preview'; cropPreview.hidden = true; const cropHandles = ['nw', 'ne', 'sw', 'se'].map((position) => { const handle = document.createElement('span'); handle.className = `image-crop-handle ${position}`; handle.dataset.cropHandle = position; handle.setAttribute('aria-label', `${t('cropHandle', '裁剪控制点')} ${position}`); cropPreview.append(handle); return handle; }); canvasWrap.append(cropPreview); const controls = document.createElement('div'); controls.className = 'preview-actions';
      let rotation = 0; let flipX = false; let flipY = false;
      const redraw = (source = image) => { const sourceWidth = source.naturalWidth || source.width; const sourceHeight = source.naturalHeight || source.height; const quarter = ((rotation % 360) + 360) % 360; const swap = quarter === 90 || quarter === 270; canvas.width = swap ? sourceHeight : sourceWidth; canvas.height = swap ? sourceWidth : sourceHeight; const ctx = canvas.getContext('2d'); ctx.save(); ctx.translate(canvas.width / 2, canvas.height / 2); ctx.rotate(quarter * Math.PI / 180); ctx.scale(flipX ? -1 : 1, flipY ? -1 : 1); ctx.drawImage(source, -sourceWidth / 2, -sourceHeight / 2); ctx.restore(); };
      const action = (icon, key, handler) => { controls.append(iconAction(icon, key, () => { snapshot(); const source = document.createElement('canvas'); source.width = canvas.width; source.height = canvas.height; source.getContext('2d').drawImage(canvas, 0, 0); rotation = 0; flipX = false; flipY = false; handler(); if (imageEditorState) imageEditorState.dirty = true; redraw(source); rotation = 0; flipX = false; flipY = false; })); };
      action('rotate-left', 'rotateLeft', () => { rotation = -90; }); action('rotate-right', 'rotateRight', () => { rotation = 90; }); action('flip-h', 'flipHorizontal', () => { flipX = true; }); action('flip-v', 'flipVertical', () => { flipY = true; });
      let tool = 'pen'; let color = '#ff3b30'; let size = 5; let drawing = false; let resizing = false; let resizeHandle = ''; let cropRect; let startX = 0; let startY = 0; const undo = []; const redo = [];
      const toolButton = (icon, value, key) => { const button = iconAction(icon, key, () => { tool = value; controls.querySelectorAll('[data-image-tool]').forEach((node) => node.dataset.active = String(node.dataset.imageTool === tool)); }); button.dataset.imageTool = value; button.dataset.active = String(value === tool); button.title = t(key, value); button.setAttribute('aria-label', button.title); controls.append(button); return button; };
      toolButton('pen', 'pen', 'imagePen'); toolButton('rect', 'rect', 'imageRectangle'); toolButton('arrow', 'arrow', 'imageArrow'); toolButton('text', 'text', 'imageText'); toolButton('crop', 'crop', 'imageCrop'); toolButton('mosaic', 'mosaic', 'imageMosaic');
      const colorInput = document.createElement('input'); colorInput.type = 'color'; colorInput.value = color; colorInput.title = t('imageColor', '画笔颜色'); colorInput.setAttribute('aria-label', colorInput.title); colorInput.oninput = () => { color = colorInput.value; }; controls.append(colorInput);
      const sizeInput = document.createElement('input'); sizeInput.type = 'range'; sizeInput.min = '1'; sizeInput.max = '60'; sizeInput.value = String(size); sizeInput.title = t('imageBrushSize', '画笔大小'); sizeInput.setAttribute('aria-label', sizeInput.title); sizeInput.oninput = () => { size = Number(sizeInput.value); }; controls.append(sizeInput);
      const restoreSnapshot = async (data) => { const restored = new Image(); restored.src = data; await restored.decode(); canvas.width = restored.naturalWidth; canvas.height = restored.naturalHeight; canvas.getContext('2d').drawImage(restored, 0, 0); if (imageEditorState) imageEditorState.dirty = true; };
      const syncHistoryButtons = () => { undoButton.disabled = !undo.length; redoButton.disabled = !redo.length; };
      const undoButton = iconAction('undo', 'undo', async () => { const previous = undo.pop(); if (!previous) return; if (redo.length >= 25) redo.shift(); redo.push(canvas.toDataURL('image/png')); await restoreSnapshot(previous); syncHistoryButtons(); }); undoButton.title = t('undo', '撤销'); undoButton.setAttribute('aria-label', undoButton.title); controls.append(undoButton);
      const redoButton = iconAction('redo', 'redo', async () => { const next = redo.pop(); if (!next) return; if (undo.length >= 25) undo.shift(); undo.push(canvas.toDataURL('image/png')); await restoreSnapshot(next); syncHistoryButtons(); }); redoButton.dataset.imageRedo = 'true'; redoButton.title = t('redo', '重做'); redoButton.setAttribute('aria-label', redoButton.title); controls.append(redoButton); syncHistoryButtons();
      const point = (event) => { const rect = canvas.getBoundingClientRect(); return { x: Math.max(0, Math.min(canvas.width, (event.clientX - rect.left) * canvas.width / rect.width)), y: Math.max(0, Math.min(canvas.height, (event.clientY - rect.top) * canvas.height / rect.height)) }; };
      const snapshot = () => { if (undo.length >= 25) undo.shift(); undo.push(canvas.toDataURL('image/png')); redo.length = 0; syncHistoryButtons(); };
      const mosaic = (x0, y0, x1, y1) => { const x = Math.floor(Math.min(x0, x1)); const y = Math.floor(Math.min(y0, y1)); const width = Math.max(1, Math.min(canvas.width - x, Math.abs(x1 - x0))); const height = Math.max(1, Math.min(canvas.height - y, Math.abs(y1 - y0))); const ctx = canvas.getContext('2d'); const pixels = ctx.getImageData(x, y, width, height); const block = Math.max(6, Math.round(Math.min(width, height) / 12)); for (let by = 0; by < height; by += block) for (let bx = 0; bx < width; bx += block) { let r = 0; let g = 0; let b = 0; let count = 0; for (let yy = by; yy < Math.min(height, by + block); yy++) for (let xx = bx; xx < Math.min(width, bx + block); xx++) { const index = (yy * width + xx) * 4; r += pixels.data[index]; g += pixels.data[index + 1]; b += pixels.data[index + 2]; count++; } for (let yy = by; yy < Math.min(height, by + block); yy++) for (let xx = bx; xx < Math.min(width, bx + block); xx++) { const index = (yy * width + xx) * 4; pixels.data[index] = r / count; pixels.data[index + 1] = g / count; pixels.data[index + 2] = b / count; } } ctx.putImageData(pixels, x, y); };
      const renderCrop = () => { if (!cropRect) { cropPreview.hidden = true; return; } const rect = canvas.getBoundingClientRect(); cropPreview.hidden = false; cropPreview.style.left = `${cropRect.x * rect.width / canvas.width}px`; cropPreview.style.top = `${cropRect.y * rect.height / canvas.height}px`; cropPreview.style.width = `${cropRect.width * rect.width / canvas.width}px`; cropPreview.style.height = `${cropRect.height * rect.height / canvas.height}px`; };
      const applyCrop = () => { if (!cropRect || cropRect.width < 2 || cropRect.height < 2) return; snapshot(); const copy = document.createElement('canvas'); copy.width = canvas.width; copy.height = canvas.height; copy.getContext('2d').drawImage(canvas, 0, 0); const next = document.createElement('canvas'); next.width = cropRect.width; next.height = cropRect.height; next.getContext('2d').drawImage(copy, cropRect.x, cropRect.y, cropRect.width, cropRect.height, 0, 0, cropRect.width, cropRect.height); canvas.width = next.width; canvas.height = next.height; canvas.getContext('2d').drawImage(next, 0, 0); cropRect = undefined; renderCrop(); if (imageEditorState) imageEditorState.dirty = true; };
      const cropApply = iconAction('check', 'applyCrop', applyCrop); cropApply.title = t('applyCrop', '应用裁剪'); cropApply.setAttribute('aria-label', cropApply.title); cropApply.hidden = true; controls.append(cropApply);
      canvas.addEventListener('pointerdown', (event) => { event.preventDefault(); const p = point(event); const handle = event.target.closest?.('[data-crop-handle]'); if (tool === 'crop' && handle && cropRect) { resizing = true; resizeHandle = handle.dataset.cropHandle; drawing = true; canvas.setPointerCapture(event.pointerId); return; } snapshot(); startX = p.x; startY = p.y; drawing = true; canvas.setPointerCapture(event.pointerId); });
      canvas.addEventListener('pointermove', (event) => { if (!drawing) return; const p = point(event); if (tool === 'crop') { if (resizing && cropRect) { const right = cropRect.x + cropRect.width; const bottom = cropRect.y + cropRect.height; if (resizeHandle.includes('w')) { cropRect.x = Math.min(p.x, right - 2); cropRect.width = right - cropRect.x; } else if (resizeHandle.includes('e')) cropRect.width = Math.max(2, p.x - cropRect.x); if (resizeHandle.includes('n')) { cropRect.y = Math.min(p.y, bottom - 2); cropRect.height = bottom - cropRect.y; } else if (resizeHandle.includes('s')) cropRect.height = Math.max(2, p.y - cropRect.y); } else cropRect = { x: Math.floor(Math.min(startX, p.x)), y: Math.floor(Math.min(startY, p.y)), width: Math.floor(Math.abs(p.x - startX)), height: Math.floor(Math.abs(p.y - startY)) }; renderCrop(); cropApply.hidden = !cropRect || cropRect.width < 2 || cropRect.height < 2; return; } const ctx = canvas.getContext('2d'); if (tool === 'pen') { ctx.strokeStyle = color; ctx.lineWidth = size; ctx.lineCap = 'round'; ctx.beginPath(); ctx.moveTo(startX, startY); ctx.lineTo(p.x, p.y); ctx.stroke(); if (imageEditorState) imageEditorState.dirty = true; startX = p.x; startY = p.y; } });
      canvas.addEventListener('pointerup', (event) => { if (!drawing) return; drawing = false; resizing = false; if (tool === 'crop') { renderCrop(); return; } const p = point(event); const ctx = canvas.getContext('2d'); if (tool === 'rect') { ctx.strokeStyle = color; ctx.lineWidth = size; ctx.strokeRect(startX, startY, p.x - startX, p.y - startY); } else if (tool === 'arrow') { const angle = Math.atan2(p.y - startY, p.x - startX); const head = Math.max(12, size * 3); ctx.strokeStyle = color; ctx.fillStyle = color; ctx.lineWidth = size; ctx.lineCap = 'round'; ctx.beginPath(); ctx.moveTo(startX, startY); ctx.lineTo(p.x, p.y); ctx.stroke(); ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(p.x - head * Math.cos(angle - 0.45), p.y - head * Math.sin(angle - 0.45)); ctx.lineTo(p.x - head * Math.cos(angle + 0.45), p.y - head * Math.sin(angle + 0.45)); ctx.closePath(); ctx.fill(); } else if (tool === 'text') {
  if (typeof link?.openModal === 'function') {
    link.openModal({
      title: t('imageTextPrompt', '输入文字'),
      label: t('text', '文字内容'),
      value: '',
      submit: (text) => {
        if (!text) return;
        ctx.fillStyle = color;
        ctx.font = `${Math.max(14, size * 4)}px sans-serif`;
        ctx.fillText(text.slice(0, 200), startX, startY);
        if (imageEditorState) imageEditorState.dirty = true;
      },
    });
  }
} else if (tool === 'mosaic') mosaic(startX, startY, p.x, p.y); });
      canvas.addEventListener('pointercancel', () => { drawing = false; });
      const format = document.createElement('select'); format.title = t('imageFormat', '输出格式'); format.setAttribute('aria-label', format.title); for (const [value, label] of [['png', 'PNG'], ['jpeg', 'JPEG'], ['webp', 'WebP']]) format.append(new Option(label, value)); controls.append(format); const quality = document.createElement('input'); quality.type = 'range'; quality.min = '10'; quality.max = '100'; quality.value = '85'; quality.hidden = true; quality.title = t('imageQuality', '输出质量'); quality.setAttribute('aria-label', quality.title); format.onchange = () => { quality.hidden = format.value === 'png'; }; controls.append(quality);
      const saveImage = async () => { try { const mime = `image/${format.value}`; const data = canvas.toDataURL(mime, Number(quality.value) / 100).split(',')[1] || ''; if (data.length > 700_000) throw new Error(t('imagePreviewUnavailable', '图片过大，无法安全保存')); const { parent } = parentAndName(item.path); const stem = parentAndName(item.path).name.replace(/\.[^.]*$/, '') || 'image'; const name = `${stem}-edited-${Date.now()}.${format.value === 'jpeg' ? 'jpg' : format.value}`; await call('write_file', { parent, name, data }); if (imageEditorState) imageEditorState.dirty = false; await loadDirectory(session.currentPath); toast(t('saved', '已保存'), 'success'); return true; } catch (error) { setStatus(error.message, 'error'); return false; } };
      const save = previewButton(t('saveAsImage', '另存为图片'), async () => { save.disabled = true; await saveImage(); save.disabled = false; }); controls.append(save); const cancel = previewButton(t('cancel', '取消'), () => { guardDirty(() => { imageEditorState = undefined; renderPreviewSelection(); }); }); controls.append(cancel); canvas.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.preventDefault(); guardDirty(() => { imageEditorState = undefined; renderPreviewSelection(); }); return; } if (!(event.metaKey || event.ctrlKey)) return; if (event.key.toLowerCase() === 'z') { event.preventDefault(); undoButton.click(); } else if (event.key.toLowerCase() === 's') { event.preventDefault(); save.click(); } }); imageEditorState = { path: item.path, dirty: false, save: saveImage }; redraw(); body.append(canvasWrap, controls);
    } catch (error) { loading.remove(); renderPreviewError(item, error.message); }
  }
  function showImageLightbox(src, alt, path) { let dialog = $('image-lightbox'); if (!dialog) { dialog = document.createElement('dialog'); dialog.id = 'image-lightbox'; dialog.onclick = (event) => { if (event.target === dialog) dialog.close(); }; dialog.addEventListener('close', () => { const image = dialog.querySelector('img'); if (image) { image.removeAttribute('src'); image.alt = ''; } delete dialog.dataset.path; delete dialog.dataset.request; }); const image = document.createElement('img'); image.className = 'lightbox-image'; image.alt = ''; dialog.append(image); document.body.append(dialog); } const image = dialog.querySelector('img'); image.src = src; image.alt = alt || ''; dialog.dataset.path = path || ''; dialog.showModal(); }
  async function renderImagePreview(item, generation) { const body = $('preview-body'); body.append(previewMeta(item)); const loading = document.createElement('p'); loading.className = 'muted'; loading.textContent = t('loading', '加载中…'); body.append(loading); const requestId = crypto.randomUUID(); session.activePreviewId = requestId; try { const result = await call('image_preview', { path: item.path }, requestId); if (generation !== session.previewGeneration || session.activePreviewId !== requestId) return; const mime = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp', 'image/bmp', 'image/avif']); if (!mime.has(result.mimeType) || typeof result.data !== 'string') throw new Error(t('imagePreviewUnavailable', '图片预览暂不可用，请使用系统应用打开')); loading.remove(); const image = document.createElement('img'); image.className = 'preview-image'; image.tabIndex = 0; image.alt = item.name; image.title = t('zoomImageHint', '滚轮缩放图片'); image.setAttribute('aria-label', `${item.name} · ${t('zoomImageHint', '滚轮缩放图片')}`); image.src = `data:${result.mimeType};base64,${result.data}`; image.onclick = () => showImageLightbox(image.src, image.alt, item.path); let scale = 1; image.addEventListener('wheel', (event) => { event.preventDefault(); scale = Math.min(4, Math.max(0.5, scale + (event.deltaY < 0 ? 0.1 : -0.1))); image.style.transform = `scale(${scale})`; image.style.cursor = scale === 1 ? 'zoom-in' : 'zoom-out'; }, { passive: false }); body.append(image); const actions = document.createElement('div'); actions.className = 'preview-actions'; const editButton = previewButton(t('editImage', '编辑图片'), () => openImageEditor(item)); editButton.replaceChildren(iconElement('pen')); editButton.appendChild(document.createTextNode(t('editImage', '编辑图片'))); actions.append(previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path)), editButton, previewButton(t('copyImage', '复制图片'), async () => { try { await call('copy_image', { path: item.path }); toast(t('imageCopied', '图片已复制')); } catch (error) { setStatus(error.message, 'error'); } })); appendCopyPathAction(actions, item); body.append(actions); } catch (error) { if (generation !== session.previewGeneration) return; loading.remove(); renderPreviewError(item, error.message === 'preview cancelled' ? t('previewCancelled', '预览已取消') : `${t('imagePreviewUnavailable', '图片预览暂不可用，请使用系统应用打开')}：${error.message}`); } finally { if (session.activePreviewId === requestId) session.activePreviewId = undefined; } }
  async function renderArchivePreview(item, generation) {
    const body = $('preview-body'); body.append(previewMeta(item)); const loading = document.createElement('p'); loading.className = 'muted'; loading.textContent = t('loading', '加载中…'); body.append(loading);
    const requestId = crypto.randomUUID(); session.activePreviewId = requestId;
    try {
      const result = await call('archive_list', { path: item.path }, requestId);
      if (generation !== session.previewGeneration || session.activePreviewId !== requestId) return;
      session.activePreviewId = undefined; loading.remove();
      if (result.unavailable) {
        const note = document.createElement('p'); note.className = 'preview-error'; note.textContent = t('archiveToolUnavailable', '系统未安装可列出此格式的受控工具，清单不可用；不支持创建与解压。'); body.append(note);
        const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.append(previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path))); appendCopyPathAction(actions, item); body.append(actions);
        return;
      }
      const meta = document.createElement('p'); meta.className = 'preview-meta muted'; meta.textContent = `${t('archiveEntries', 'Archive entries')}: ${result.totalEntries ?? result.entries?.length ?? 0}`; body.append(meta);
      const list = document.createElement('ul'); list.className = 'archive-entries';
      for (const entry of result.entries || []) {
        const row = document.createElement('li'); row.className = 'archive-entry';
        const name = document.createElement('span'); name.className = 'usage-entry-label'; name.append(entryIcon({ ...entry, isDir: Boolean(entry.isDir) }), document.createTextNode(` ${entry.name}`));
        const size = document.createElement('span'); size.className = 'entry-meta size'; size.textContent = entry.isDir ? t('folder', '文件夹') : formatSize(Number(entry.size) || 0);
        row.append(name, size); list.append(row);
      }
      body.append(list);
      const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.append(previewButton(t('extractArchive', '解压'), () => link.extractArchive(item)), previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path))); appendCopyPathAction(actions, item); body.append(actions);
    } catch (error) {
      if (generation !== session.previewGeneration) return;
      loading.remove(); renderPreviewError(item, error.message === 'preview cancelled' ? t('previewCancelled', '预览已取消') : `${t('archivePreviewUnavailable', '无法安全读取压缩包清单')}：${error.message}`);
    } finally { if (session.activePreviewId === requestId) session.activePreviewId = undefined; }
  }
  async function renderPdfPreview(item, generation) {
    const body = $('preview-body'); body.append(previewMeta(item)); const loading = document.createElement('p'); loading.className = 'muted'; loading.textContent = t('loading', '加载中…'); body.append(loading);
    const requestId = crypto.randomUUID(); session.activePreviewId = requestId;
    try {
      const result = await call('pdf_preview', { path: item.path }, requestId);
      if (generation !== session.previewGeneration || session.activePreviewId !== requestId) return;
      const binary = atob(result.data || ''); const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
      if (bytes.length < 5 || new TextDecoder().decode(bytes.slice(0, 5)) !== '%PDF-') throw new Error(t('pdfPreviewUnavailable', 'PDF 预览暂不可用'));
      const pdfText = new TextDecoder('latin1').decode(bytes); const pageMatches = pdfText.match(/\/Type\s*\/Page(?!s)\b/g); const totalPages = Math.min(10_000, pageMatches?.length || 0);
      if (session.previewObjectUrl) URL.revokeObjectURL(session.previewObjectUrl); session.previewObjectUrl = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' })); loading.remove();
      let page = 1; const pageLabel = document.createElement('span'); pageLabel.className = 'pdf-page-label';
      const previewObjectUrl = session.previewObjectUrl;
      const frame = document.createElement('iframe'); frame.className = 'pdf-preview'; frame.dataset.totalPages = String(totalPages); frame.title = item.name; frame.setAttribute('aria-label', `${item.name} · ${t('pdfViewerHint', 'PDF viewer')}`); frame.setAttribute('sandbox', 'allow-same-origin'); frame.src = `${previewObjectUrl}#page=1&zoom=page-width`; body.append(frame);
      const setPage = (next) => { page = Math.max(1, Math.min(totalPages || Number.MAX_SAFE_INTEGER, next)); frame.src = `${previewObjectUrl}#page=${page}&zoom=page-width`; previous.disabled = page <= 1; nextButton.disabled = totalPages > 0 && page >= totalPages; pageLabel.textContent = `${t('pdfPage', '当前页')} ${page}${totalPages ? ` / ${totalPages}` : ''}`; };
      const previous = previewButton(t('pdfPrevious', '上一页'), () => setPage(page - 1)); previous.dataset.action = 'pdfPrevious';
      const nextButton = previewButton(t('pdfNext', '下一页'), () => setPage(page + 1)); nextButton.dataset.action = 'pdfNext';
      pageLabel.setAttribute('aria-live', 'polite');
      const findButton = previewButton(t('search', '搜索'), () => { frame.focus(); setStatus(`${t('pdfViewerHint', 'PDF viewer')} · ${t('search', '搜索')} ⌘/Ctrl+F`); }); findButton.dataset.action = 'pdf-find'; findButton.title = `${t('search', '搜索')} (⌘/Ctrl+F)`; findButton.setAttribute('aria-label', findButton.title);
      const pageActions = document.createElement('div'); pageActions.className = 'preview-actions pdf-page-actions'; pageActions.append(findButton, previous, pageLabel, nextButton); body.append(pageActions); setPage(1);
      const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.append(previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path))); appendCopyPathAction(actions, item); body.append(actions);
    } catch (error) { if (generation !== session.previewGeneration) return; loading.remove(); renderPreviewError(item, `${t('pdfPreviewUnavailable', 'PDF 预览暂不可用')}：${error.message}`); }
    finally { if (session.activePreviewId === requestId) session.activePreviewId = undefined; }
  }
  async function renderMediaPreview(item, generation) {
    const body = $('preview-body'); body.append(previewMeta(item)); const loading = document.createElement('p'); loading.className = 'muted'; loading.textContent = t('loading', '加载中…'); body.append(loading);
    const requestId = crypto.randomUUID(); session.activePreviewId = requestId;
    try {
      const result = await call('media_preview', { path: item.path }, requestId);
      if (generation !== session.previewGeneration || session.activePreviewId !== requestId) return;
      const binary = atob(result.data || ''); const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
      if (!/^audio\/|^video\//.test(result.mimeType || '')) throw new Error(t('mediaPreviewUnavailable', '音视频预览暂不可用'));
      if (session.previewObjectUrl) URL.revokeObjectURL(session.previewObjectUrl); session.previewObjectUrl = URL.createObjectURL(new Blob([bytes], { type: result.mimeType })); loading.remove();
      const media = document.createElement(result.mimeType.startsWith('audio/') ? 'audio' : 'video'); media.className = 'media-preview'; media.controls = true; media.preload = 'metadata'; media.src = session.previewObjectUrl; media.setAttribute('aria-label', item.name); media.addEventListener('error', () => { if (generation !== session.previewGeneration) return; media.remove(); renderPreviewError(item, t('mediaPreviewUnavailable', '音视频预览暂不可用')); }); body.append(media);
      const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.append(previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path))); appendCopyPathAction(actions, item); body.append(actions);
    } catch (error) { if (generation !== session.previewGeneration) return; loading.remove(); renderPreviewError(item, `${t('mediaPreviewUnavailable', '音视频预览暂不可用')}：${error.message}`); }
    finally { if (session.activePreviewId === requestId) session.activePreviewId = undefined; }
  }
  function setPreviewOpen(open) {
    $('preview').hidden = !open;
    document.querySelector('.layout').classList.toggle('preview-open', open);
  }
  async function renderPreviewSelection(options = {}) {
    const previousEditor = $('preview-body').querySelector('.file-editor'); if (editorState?.path && previousEditor) { editorViewStates.set(editorState.path, { scrollTop: previousEditor.scrollTop, selectionStart: previousEditor.selectionStart, selectionEnd: previousEditor.selectionEnd }); while (editorViewStates.size > 20) editorViewStates.delete(editorViewStates.keys().next().value); }
    const generation = ++session.previewGeneration; editorRefreshToken++; if (session.activePreviewId) { call('preview_cancel', { requestId: session.activePreviewId }).catch(() => {}); session.activePreviewId = undefined; } if (session.previewObjectUrl) { URL.revokeObjectURL(session.previewObjectUrl); session.previewObjectUrl = undefined; }
    const item = selectedItems()[0]; clearPreviewBody(); if (!item || item.isDir) { setPreviewOpen(false); const note = document.createElement('p'); note.className = 'muted'; note.textContent = t('selectPreview', '选择文件查看预览'); $('preview-body').append(note); editorState = undefined; return; }
    setPreviewOpen(true);
    if (item.kind === 'archive' && /\.(zip|jar|tar|gz|tgz|tbz2?|txz|tar\.(gz|bz2|xz|zst)|tzst)$/i.test(item.name)) { editorState = undefined; await renderArchivePreview(item, generation); return; }
    if (item.kind === 'pdf') { editorState = undefined; await renderPdfPreview(item, generation); return; }
    if (item.kind === 'audio' || item.kind === 'video') { editorState = undefined; await renderMediaPreview(item, generation); return; }
    if (item.kind === 'image') { editorState = undefined; await renderImagePreview(item, generation); return; }
    if (!isTextItem(item)) { renderUnsupportedPreview(item); editorState = undefined; return; }
    const loading = document.createElement('p'); loading.className = 'muted'; loading.textContent = t('loading', '加载中…'); $('preview-body').append(loading); const requestId = crypto.randomUUID(); session.activePreviewId = requestId;
    try { const result = await call('read_file', { path: item.path }, requestId); if (generation !== session.previewGeneration || session.activePreviewId !== requestId) return; session.activePreviewId = undefined; loading.remove(); if (!TEXT_KINDS.has(result.kind) && !isTextItem(item)) { renderUnsupportedPreview(item); return; } if (/\.html?$/i.test(item.name)) { editorState = undefined; renderHtmlPreview(item, result.content || '', $('preview-body')); } else { renderTextEditor(item, result, options); appendEditorPreviewActions(item); } await remember(item.path); } catch (error) { if (generation !== session.previewGeneration) return; loading.remove(); renderPreviewError(item, error.message === 'preview cancelled' ? t('previewCancelled', '预览已取消') : `${t('readFailed', '读取文件失败')}：${error.message}`); } finally { if (session.activePreviewId === requestId) session.activePreviewId = undefined; }
  }
  function markdownImagePaths(content, parent) {
    const paths = []; const seen = new Set();
    for (const match of String(content || '').matchAll(/!\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)) {
      const rawValue = match[1].trim(); let raw; try { raw = decodeURIComponent(rawValue); } catch { raw = rawValue; }
      if (!raw || /^[\\/]/.test(raw) || /^[A-Za-z]:[\\/]/.test(raw) || raw.startsWith('~') || raw.includes('://')) continue;
      const segments = [...pathParts(parent.replaceAll('\\', '/')), ...raw.replaceAll('\\', '/').split('/')]; const normalized = [];
      let escaped = false;
      for (const segment of segments) { if (!segment || segment === '.') continue; if (segment === '..') { if (!normalized.length) { escaped = true; break; } normalized.pop(); } else normalized.push(segment); }
      if (escaped || !normalized.length) continue;
      const path = `/${normalized.join('/')}`;
      if (!seen.has(path)) { seen.add(path); paths.push(path); }
      if (paths.length >= 8) break;
    }
    return paths;
  }
  async function renderMarkdownImages(item, content, body, generation) {
    const paths = markdownImagePaths(content, parentAndName(item.path).parent);
    if (!paths.length) return;
    const section = document.createElement('section'); section.className = 'markdown-images';
    const heading = document.createElement('p'); heading.className = 'preview-meta muted'; heading.textContent = t('markdownImages', 'Markdown images'); const loading = document.createElement('p'); loading.className = 'muted'; loading.textContent = t('loading', '加载中…'); section.append(heading, loading);
    const images = await Promise.all(paths.map(async (path) => {
      try { const result = await call('image_preview', { path }, crypto.randomUUID()); if (!result?.mimeType || typeof result.data !== 'string') return undefined; const image = document.createElement('img'); image.className = 'preview-image'; image.tabIndex = 0; image.alt = path.split('/').pop() || path; image.loading = 'lazy'; image.decoding = 'async'; image.src = `data:${result.mimeType};base64,${result.data}`; image.onclick = () => showImageLightbox(image.src, image.alt, path); return image; } catch { return undefined; }
    }));
    if (generation !== session.previewGeneration || editorState?.path !== item.path) return;
    loading.remove(); images.filter(Boolean).forEach((image) => section.append(image)); section.hidden = body.querySelector('.markdown-mode-toggle')?.dataset.mode !== 'reading'; if (section.querySelector('img')) body.append(section);
  }
  function renderInlineMarkdown(source, container) {
    if (!source) return;
    const pattern = /(`[^`]+`|!?\[[^\]]*\]\([^) \t\r\n]+(?:\s+["'][^"']*["'])?\)|(?:\*\*|__)(?:[^*_]+|\*(?!\*)|\_(?!\_))+(?:\*\*|__)|~~[^~]+~~|(?:\*|_)(?:[^*_]+)+(?:\*|_))/g;
    let lastIndex = 0; let match;
    while ((match = pattern.exec(source)) !== null) {
      if (match.index > lastIndex) container.append(document.createTextNode(source.slice(lastIndex, match.index)));
      const token = match[0]; lastIndex = pattern.lastIndex;
      if (token.startsWith('`') && token.endsWith('`') && token.length >= 2) {
        const code = document.createElement('code'); code.textContent = token.slice(1, -1); container.append(code); continue;
      }
      if (token.startsWith('![') || token.startsWith('[')) {
        const isImg = token.startsWith('!');
        const linkMatch = token.match(/^!?\[([^\]]*)\]\(([^)\s]+)(?:\s+["']([^"']*)["'])?\)$/);
        if (linkMatch) {
          const text = linkMatch[1]; const rawUrl = linkMatch[2];
          if (isImg) {
            const img = document.createElement('img'); img.className = 'markdown-inline-image'; img.alt = text; img.title = linkMatch[3] || text;
            if (/^(https?:|data:image\/)/i.test(rawUrl)) img.src = rawUrl;
            container.append(img);
          } else if (/^https?:\/\//i.test(rawUrl)) {
            const a = document.createElement('a'); a.href = rawUrl; a.textContent = text || rawUrl; a.target = '_blank'; a.rel = 'noopener noreferrer'; container.append(a);
          } else container.append(document.createTextNode(token));
          continue;
        }
      }
      if ((token.startsWith('**') && token.endsWith('**')) || (token.startsWith('__') && token.endsWith('__'))) {
        const strong = document.createElement('strong'); renderInlineMarkdown(token.slice(2, -2), strong); container.append(strong); continue;
      }
      if (token.startsWith('~~') && token.endsWith('~~')) {
        const del = document.createElement('del'); renderInlineMarkdown(token.slice(2, -2), del); container.append(del); continue;
      }
      if ((token.startsWith('*') && token.endsWith('*')) || (token.startsWith('_') && token.endsWith('_'))) {
        const em = document.createElement('em'); renderInlineMarkdown(token.slice(1, -1), em); container.append(em); continue;
      }
      container.append(document.createTextNode(token));
    }
    if (lastIndex < source.length) container.append(document.createTextNode(source.slice(lastIndex)));
  }
  function renderMarkdownSafePreview(item, content, body, generation) {
    if (!/\.(md|markdown|mdx)$/i.test(item.name)) return;
    const section = document.createElement('section'); section.className = 'markdown-preview';
    const heading = document.createElement('p'); heading.className = 'preview-meta muted'; heading.textContent = t('markdownPreview', 'Markdown preview'); section.append(heading);
    let fenced = false; let codeBlock; let currentList = null; let currentListType = null; let currentTable = null; let nodes = 0;
    const lines = String(content || '').slice(0, 128 * 1024).split(/\r?\n/);
    for (let i = 0; i < lines.length && nodes < 1000; i++) {
      const line = lines[i].slice(0, 4_000);
      if (/^\s*```/.test(line)) {
        fenced = !fenced; currentList = null; currentTable = null;
        if (fenced) {
          codeBlock = document.createElement('pre'); const code = document.createElement('code');
          const langMatch = line.match(/^\s*```([a-zA-Z0-9_-]+)/); if (langMatch) code.className = `language-${langMatch[1]}`;
          codeBlock.append(code); section.append(codeBlock); nodes++;
        }
        continue;
      }
      if (fenced) { const code = codeBlock.querySelector('code') || codeBlock; code.textContent += `${line}\n`; continue; }
      const trimmed = line.trim();
      if (!trimmed) { currentList = null; currentTable = null; continue; }
      if (/^(?:-{3,}|\*{3,}|_{3,})$/.test(trimmed)) { currentList = null; currentTable = null; section.append(document.createElement('hr')); nodes++; continue; }
      const headerMatch = line.match(/^\s*(#{1,6})\s+(.+)$/);
      if (headerMatch) {
        currentList = null; currentTable = null;
        const h = document.createElement(`h${headerMatch[1].length}`); renderInlineMarkdown(headerMatch[2], h); section.append(h); nodes++; continue;
      }
      const quoteMatch = line.match(/^\s*>\s?(.*)$/);
      if (quoteMatch) {
        currentList = null; currentTable = null;
        const quote = document.createElement('blockquote'); renderInlineMarkdown(quoteMatch[1], quote); section.append(quote); nodes++; continue;
      }
      const taskMatch = line.match(/^\s*[-*+]\s+\[([ xX])\]\s+(.*)$/);
      if (taskMatch) {
        currentTable = null;
        if (currentListType !== 'task' || !currentList) {
          currentList = document.createElement('ul'); currentList.className = 'markdown-task-list'; currentListType = 'task'; section.append(currentList);
        }
        const li = document.createElement('li'); li.className = 'markdown-task-item';
        const checkbox = document.createElement('input'); checkbox.type = 'checkbox'; checkbox.disabled = true; checkbox.checked = taskMatch[1].toLowerCase() === 'x';
        const textSpan = document.createElement('span'); renderInlineMarkdown(taskMatch[2], textSpan);
        li.append(checkbox, textSpan); currentList.append(li); nodes++; continue;
      }
      const unorderedMatch = line.match(/^\s*[-*+]\s+(.+)$/);
      if (unorderedMatch) {
        currentTable = null;
        if (currentListType !== 'ul' || !currentList) {
          currentList = document.createElement('ul'); currentListType = 'ul'; section.append(currentList);
        }
        const li = document.createElement('li'); renderInlineMarkdown(unorderedMatch[1], li); currentList.append(li); nodes++; continue;
      }
      const orderedMatch = line.match(/^\s*(\d+)\.\s+(.+)$/);
      if (orderedMatch) {
        currentTable = null;
        if (currentListType !== 'ol' || !currentList) {
          currentList = document.createElement('ol'); currentListType = 'ol'; section.append(currentList);
        }
        const li = document.createElement('li'); renderInlineMarkdown(orderedMatch[2], li); currentList.append(li); nodes++; continue;
      }
      if (trimmed.startsWith('|') && trimmed.endsWith('|') && trimmed.includes('|')) {
        currentList = null; const cells = trimmed.slice(1, -1).split('|').map((c) => c.trim());
        if (cells.every((c) => /^:?-{3,}:?$/.test(c))) continue;
        if (!currentTable) {
          currentTable = document.createElement('table'); currentTable.className = 'markdown-table';
          const thead = document.createElement('thead'); const tr = document.createElement('tr');
          cells.forEach((cell) => { const th = document.createElement('th'); renderInlineMarkdown(cell, th); tr.append(th); });
          thead.append(tr); currentTable.append(thead, document.createElement('tbody')); section.append(currentTable); nodes++; continue;
        }
        const tbody = currentTable.querySelector('tbody');
        if (tbody) {
          const tr = document.createElement('tr');
          cells.forEach((cell) => { const td = document.createElement('td'); renderInlineMarkdown(cell, td); tr.append(td); });
          tbody.append(tr); nodes++; continue;
        }
      }
      currentList = null; currentTable = null;
      const p = document.createElement('p'); renderInlineMarkdown(trimmed, p); section.append(p); nodes++;
    }
    if (generation === session.previewGeneration && editorState?.path === item.path) body.append(section);
  }
  function parseCsvRows(content) {
    const rows = []; let row = []; let cell = ''; let quoted = false; const source = String(content || '').slice(0, 64 * 1024);
    for (let index = 0; index < source.length && rows.length < 200; index++) {
      const character = source[index];
      if (character === '"') { if (quoted && source[index + 1] === '"') { cell += '"'; index++; } else quoted = !quoted; continue; }
      if (!quoted && character === ',') { row.push(cell.slice(0, 2_000)); cell = ''; continue; }
      if (!quoted && (character === '\n' || character === '\r')) { if (character === '\r' && source[index + 1] === '\n') index++; row.push(cell.slice(0, 2_000)); rows.push(row); row = []; cell = ''; continue; }
      cell += character;
    }
    if (row.length || cell) { row.push(cell.slice(0, 2_000)); rows.push(row); }
    return rows.map((values) => values.slice(0, 32));
  }
  function renderCsvPreview(item, content, body, generation) {
    if (!/\.csv$/i.test(item.name)) return;
    const rows = parseCsvRows(content); if (!rows.length) return;
    const section = document.createElement('section'); section.className = 'csv-preview';
    const heading = document.createElement('p'); heading.className = 'preview-meta muted'; heading.textContent = t('csvPreview', 'CSV preview'); section.append(heading);
    const table = document.createElement('table'); const head = document.createElement('thead'); const bodyRows = document.createElement('tbody');
    rows[0].forEach((value) => { const cell = document.createElement('th'); cell.textContent = value; head.append(cell); }); table.append(head);
    for (const values of rows.slice(1)) { const row = document.createElement('tr'); for (let index = 0; index < rows[0].length; index++) { const cell = document.createElement('td'); cell.textContent = values[index] || ''; row.append(cell); } bodyRows.append(row); }
    table.append(bodyRows); section.append(table); if (generation === session.previewGeneration && editorState?.path === item.path) body.append(section);
  }
  function stripJsoncComments(content) {
    let output = ''; let quoted = false; let escaped = false; let lineComment = false; let blockComment = false; const source = String(content || '');
    for (let index = 0; index < source.length; index++) {
      const character = source[index]; const next = source[index + 1];
      if (lineComment) { if (character === '\n' || character === '\r') { lineComment = false; output += character; } continue; }
      if (blockComment) { if (character === '*' && next === '/') { blockComment = false; index++; } else if (character === '\n' || character === '\r') output += character; continue; }
      if (quoted) { output += character; if (escaped) escaped = false; else if (character === '\\') escaped = true; else if (character === '"') quoted = false; continue; }
      if (character === '"') { quoted = true; output += character; continue; }
      if (character === '/' && next === '/') { lineComment = true; index++; continue; }
      if (character === '/' && next === '*') { blockComment = true; index++; continue; }
      output += character;
    }
    return output;
  }
  function stripJsoncTrailingCommas(content) {
    let output = ''; let quoted = false; let escaped = false; const source = String(content || '');
    for (let index = 0; index < source.length; index++) {
      const character = source[index];
      if (quoted) { output += character; if (escaped) escaped = false; else if (character === '\\') escaped = true; else if (character === '"') quoted = false; continue; }
      if (character === '"') { quoted = true; output += character; continue; }
      if (character === ',') { let lookahead = index + 1; while (/\s/.test(source[lookahead] || '')) lookahead++; if (source[lookahead] === ']' || source[lookahead] === '}') continue; }
      output += character;
    }
    return output;
  }
  function renderJsonPreview(item, content, body, generation) {
    if (!/\.jsonc?$/i.test(item.name)) return;
    const source = stripJsoncTrailingCommas(stripJsoncComments(String(content || '').slice(0, 64 * 1024)));
    let value; try { value = JSON.parse(source); } catch { return; }
    const section = document.createElement('section'); section.className = 'json-preview';
    const heading = document.createElement('p'); heading.className = 'preview-meta muted'; heading.textContent = t('jsonPreview', 'JSON preview'); section.append(heading);
    const output = document.createElement('pre'); output.textContent = JSON.stringify(value, null, 2).slice(0, 96 * 1024); section.append(output);
    if (generation === session.previewGeneration && editorState?.path === item.path) body.append(section);
  }
  function renderHtmlPreview(item, content, body) {
    editorState = { path: item.path, mtime: Number(item.mtime) || 0, savedContent: String(content || ''), dirty: false, htmlPreview: true };
    const meta = document.createElement('p'); meta.className = 'preview-meta muted'; meta.textContent = t('htmlPreview', 'HTML preview (scripts disabled)');
    const frame = document.createElement('iframe'); frame.className = 'html-preview'; frame.title = item.name; frame.setAttribute('sandbox', 'allow-same-origin');
    const policy = '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; img-src data: blob:; style-src \'unsafe-inline\'">';
    session.previewObjectUrl = URL.createObjectURL(new Blob([policy, String(content || '')], { type: 'text/html' })); frame.src = session.previewObjectUrl; body.append(meta, frame); const actions = document.createElement('div'); actions.className = 'preview-actions'; actions.append(previewButton(t('open', '打开'), () => link.openItem(item)), previewButton(t('reveal', '显示'), () => revealPath(item.path)), previewButton(t('openEditor', '在编辑器打开'), () => call('editor', { path: item.path }).catch((error) => setStatus(error.message, 'error')))); appendCopyPathAction(actions, item); body.append(actions);
  }
  function renderTextEditor(item, result, options = {}) {
    const readOnly = Boolean(result.truncated); const generation = session.previewGeneration; const isMarkdown = /\.(md|markdown|mdx)$/i.test(item.name); const initialReading = isMarkdown && !options.editMode; editorState = { path: item.path, mtime: Number(result.mtime) || Number(item.mtime) || 0, savedContent: result.content || '', dirty: false, saveTimer: undefined, conflict: false, readOnly }; const body = $('preview-body'); const meta = document.createElement('p'); meta.className = 'preview-meta muted'; meta.textContent = `${formatSize(result.size)} · ${result.encoding || 'utf-8'}${readOnly ? ` · ${t('previewTruncated', '内容已截断')}` : ''}`; const toolbar = document.createElement('div'); toolbar.className = 'editor-toolbar'; if (isMarkdown) toolbar.dataset.mode = initialReading ? 'reading' : 'source'; const leftGroup = document.createElement('div'); leftGroup.className = 'editor-toolbar-left'; const stateLabel = document.createElement('span'); stateLabel.className = 'editor-save-status muted'; stateLabel.textContent = readOnly ? t('largeTextReadOnly', '大文件仅显示预览，编辑已禁用') : t('saved', '已保存'); stateLabel.hidden = initialReading || readOnly; leftGroup.append(stateLabel); const rightGroup = document.createElement('div'); rightGroup.className = 'editor-toolbar-right'; const save = document.createElement('button'); save.className = 'editor-save-btn'; save.textContent = t('save', '保存'); save.disabled = true; save.hidden = initialReading || readOnly; save.onclick = () => queueEditorSave(true); const toggle = document.createElement('button'); toggle.className = 'markdown-mode-toggle'; toggle.hidden = !isMarkdown; toggle.dataset.mode = initialReading ? 'reading' : 'source'; toggle.textContent = t(initialReading ? 'edit' : 'preview', initialReading ? '编辑' : '预览'); toggle.onclick = () => { const reading = toggle.dataset.mode !== 'reading'; toggle.dataset.mode = reading ? 'reading' : 'source'; toggle.textContent = t(reading ? 'edit' : 'preview', reading ? '编辑' : '预览'); toolbar.dataset.mode = reading ? 'reading' : 'source'; editor.hidden = reading; stateLabel.hidden = reading || readOnly; save.hidden = reading || readOnly; if (reading) { body.querySelectorAll('.markdown-preview,.markdown-images').forEach((node) => node.remove()); renderMarkdownSafePreview(item, editor.value || '', body, generation); renderMarkdownImages(item, editor.value || '', body, generation); } else { body.querySelectorAll('.markdown-preview,.markdown-images').forEach((node) => { node.hidden = true; }); editor.focus(); } }; rightGroup.append(save, toggle); toolbar.append(leftGroup, rightGroup); const editor = document.createElement('textarea'); editor.className = 'file-editor'; editor.value = result.content || ''; editor.readOnly = readOnly; editor.spellcheck = false; editor.hidden = initialReading; editor.setAttribute('aria-label', t('editorLabel', '文件编辑器')); editor.addEventListener('input', () => { if (!editorState || editorState.readOnly) return; editorState.dirty = editor.value !== editorState.savedContent; editorState.conflict = false; stateLabel.textContent = editorState.dirty ? t('unsaved', '未保存') : t('saved', '已保存'); save.disabled = !editorState.dirty; scheduleEditorSave(); }); editor.addEventListener('keydown', (event) => { if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') { event.preventDefault(); if (!editorState?.readOnly) queueEditorSave(true); } }); body.append(meta, toolbar, editor); renderJsonPreview(item, result.content || '', body, generation); renderCsvPreview(item, result.content || '', body, generation); renderMarkdownSafePreview(item, result.content || '', body, generation); renderMarkdownImages(item, result.content || '', body, generation); if (!initialReading) { body.querySelectorAll('.markdown-preview,.markdown-images').forEach((node) => { node.hidden = true; }); editor.focus(); } const view = editorViewStates.get(item.path); if (view) { editor.scrollTop = Math.max(0, Number(view.scrollTop) || 0); editor.setSelectionRange(Math.min(Number(view.selectionStart) || 0, editor.value.length), Math.min(Number(view.selectionEnd) || 0, editor.value.length)); }
  }
  function resetPreview() { if (editorState?.dirty) { guardDirty(() => resetPreviewNow()); return; } resetPreviewNow(); }
  function resetPreviewNow() { rememberEditorViewState(); if (session.activePreviewId && isHostConnected()) call('preview_cancel', { requestId: session.activePreviewId }).catch(() => {}); session.activePreviewId = undefined; if (session.previewObjectUrl) { URL.revokeObjectURL(session.previewObjectUrl); session.previewObjectUrl = undefined; } $('image-lightbox')?.close(); session.previewGeneration++; editorState = undefined; imageEditorState = undefined; session.selectedPaths.clear(); renderSelection(); clearPreviewBody(); const note = document.createElement('p'); note.className = 'muted'; note.textContent = t('selectPreview', '选择文件查看预览'); $('preview-body').append(note); }

  function scheduleEditorSave() { if (!editorState?.dirty) return; clearTimeout(editorState.saveTimer); editorState.saveTimer = setTimeout(() => queueEditorSave(false), 800); }
  function queueEditorSave(manual = false) { if (!editorState?.dirty) return Promise.resolve(true); const target = editorState; saveQueue = saveQueue.catch(() => {}).then(() => saveEditorNow(target, manual)); return saveQueue; }
  async function saveEditorNow(state, manual = false, force = false) {
    if (!state || state !== editorState || state.readOnly || !state.dirty) return true; const editor = $('preview-body').querySelector('.file-editor'); if (!editor) return false; const content = editor.value; const { parent, name } = parentAndName(state.path);
    const label = $('preview-body').querySelector('.editor-toolbar .muted'); if (label) label.textContent = t('processing', '保存中…');
    try { const result = await call('write_file', { parent, name, data: encodeBase64(content), ...(force ? {} : { expectedMtime: state.mtime }) }); if (state !== editorState) return false; if (result?.conflict && !force) { state.conflict = true; setStatus(t('editConflict', '文件已被外部修改'), 'error'); const choice = await askConflict(); if (choice === 'overwrite') return saveEditorNow(state, manual, true); if (choice === 'reload') { await reloadEditor(state); return true; } return false; } state.mtime = Number(result?.mtime) || state.mtime; state.savedContent = content; state.dirty = editor.value !== content; state.conflict = false; const save = $('preview-body').querySelector('.editor-toolbar button'); if (save) save.disabled = !state.dirty; if (label) label.textContent = state.dirty ? t('unsaved', '未保存') : t('saved', '已保存'); setStatus(state.dirty ? t('unsaved', '未保存') : t('saved', '已保存'), state.dirty ? '' : 'success'); if (state.dirty) scheduleEditorSave(); else if (manual) toast(t('saved', '已保存'), 'success'); return !state.dirty; } catch (error) { if (state === editorState) { if (label) label.textContent = t('unsaved', '未保存'); setStatus(`${t('saveFailed', '保存失败')}：${error.message}`, 'error'); } return false; }
  }
  async function reloadEditor(state) { const result = await call('read_file', { path: state.path }, crypto.randomUUID()); if (state !== editorState) return false; state.mtime = Number(result.mtime) || state.mtime; state.savedContent = result.content || ''; state.dirty = false; state.conflict = false; const editor = $('preview-body').querySelector('.file-editor'); if (editor) editor.value = state.savedContent; const save = $('preview-body').querySelector('.editor-toolbar button'); if (save) save.disabled = true; const label = $('preview-body').querySelector('.editor-toolbar .muted'); if (label) label.textContent = t('saved', '已保存'); toast(t('reloaded', '已重新加载')); return true; }
  function askConflict() { return new Promise((resolve) => { const dialog = $('conflict-modal'); const finish = () => { dialog.onclose = null; resolve(dialog.returnValue || 'cancel'); }; dialog.onclose = finish; dialog.showModal(); }); }
  function askDirty() { return new Promise((resolve) => { const dialog = $('dirty-modal'); const finish = () => { dialog.onclose = null; resolve(dialog.returnValue || 'cancel'); }; dialog.onclose = finish; dialog.showModal(); }); }
  async function guardDirty(action) { if (imageEditorState?.dirty) { const choice = await askDirty(); if (choice === 'save') { if (!await imageEditorState.save()) return false; } else if (choice === 'discard') imageEditorState = undefined; else return false; } if (!editorState?.dirty) return action(); const choice = await askDirty(); if (choice === 'save') { if (!await queueEditorSave(true)) return false; } else if (choice === 'discard') { clearTimeout(editorState.saveTimer); editorState = undefined; } else return false; return action(); }
  async function refreshEditorAfterExternalChange(path) { if (!editorState || editorState.path !== path) return; const state = editorState; const refreshToken = ++editorRefreshToken; const editor = $('preview-body').querySelector('.file-editor'); const scrollTop = editor?.scrollTop || 0; const selectionStart = editor?.selectionStart || 0; const selectionEnd = editor?.selectionEnd || 0; try { const result = await call('read_file', { path }, crypto.randomUUID()); if (refreshToken !== editorRefreshToken || state !== editorState || state.path !== path) return; if (state.htmlPreview) { const frame = $('preview-body').querySelector('.html-preview'); if (!frame) return; if (session.previewObjectUrl) URL.revokeObjectURL(session.previewObjectUrl); const policy = '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; img-src data: blob:; style-src \'unsafe-inline\'">'; session.previewObjectUrl = URL.createObjectURL(new Blob([policy, result.content || ''], { type: 'text/html' })); frame.src = session.previewObjectUrl; state.mtime = Number(result.mtime) || state.mtime; state.savedContent = result.content || ''; return; } if (state.dirty) { state.conflict = true; setStatus(t('editConflict', '文件已被外部修改'), 'error'); return; } state.mtime = Number(result.mtime) || state.mtime; state.savedContent = result.content || ''; if (editor) { editor.value = state.savedContent; editor.scrollTop = scrollTop; editor.setSelectionRange(Math.min(selectionStart, editor.value.length), Math.min(selectionEnd, editor.value.length)); } } catch { /* directory refresh reports inaccessible files */ } }

  return { renderPreviewSelection, resetPreview, resetPreviewNow, guardDirty, refreshEditorAfterExternalChange, migrateEditorViewPath, getEditorState, getImageEditorState, hasDirtyEditor, showImageLightbox };
}
