/**
 * FilesPreviewPanel Component
 * Manages the right preview sidebar: layout toggle (side/bottom/maximized),
 * resizer dragging, text editing/saving, image editing/lightbox, PDF pagination,
 * archive listings, and markdown/HTML/CSV/JSON viewers.
 */
export class FilesPreviewPanel {
  constructor({
    container,
    body,
    resizer,
    layoutButton,
    maximizeButton,
    closeButton,
    initialWidth = 360,
    initialHeight = 320,
    initialBottom = false,
    call,
    t = (key, fallback) => fallback || key,
    entryIcon,
    iconElement,
    formatSize,
    onStatus,
    onToast,
    onOpenItem,
    onRevealItem,
    onLayoutChange,
    onRememberPath,
    onDirectoryReload,
  }) {
    this.container = typeof container === 'string' ? document.querySelector(container) : container;
    this.body = typeof body === 'string' ? document.querySelector(body) : (body || this.container?.querySelector('#preview-body'));
    this.resizer = typeof resizer === 'string' ? document.querySelector(resizer) : (resizer || this.container?.querySelector('#preview-resizer'));
    this.layoutButton = typeof layoutButton === 'string' ? document.querySelector(layoutButton) : (layoutButton || this.container?.querySelector('#toggle-preview-layout'));
    this.maximizeButton = typeof maximizeButton === 'string' ? document.querySelector(maximizeButton) : (maximizeButton || this.container?.querySelector('#maximize-preview'));
    this.closeButton = typeof closeButton === 'string' ? document.querySelector(closeButton) : (closeButton || this.container?.querySelector('#close-preview'));

    this.width = Math.min(620, Math.max(240, Number(initialWidth) || 360));
    this.height = Math.min(600, Math.max(180, Number(initialHeight) || 320));
    this.bottom = Boolean(initialBottom);
    this.maximized = false;
    this.isOpen = false;

    this.call = call;
    this.t = t;
    this.entryIcon = entryIcon;
    this.iconElement = iconElement || ((name) => {
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      svg.setAttribute('class', 'icon');
      svg.innerHTML = `<use href="#i-${name}"/>`;
      return svg;
    });
    this.formatSize = formatSize || ((s) => `${s} B`);
    this.onStatus = onStatus;
    this.onToast = onToast;
    this.onOpenItem = onOpenItem;
    this.onRevealItem = onRevealItem;
    this.onLayoutChange = onLayoutChange;
    this.onRememberPath = onRememberPath;
    this.onDirectoryReload = onDirectoryReload;

    this.activePreviewId = undefined;
    this.previewGeneration = 0;
    this.previewObjectUrl = undefined;
    this.editorState = undefined;
    this.imageEditorState = undefined;
    this.editorViewStates = new Map();
    this.resizing = false;

    this._init();
  }

  _init() {
    this._bindControls();
    this._syncLayout();
  }

  _bindControls() {
    if (this.closeButton) {
      this.closeButton.onclick = () => this.close();
    }

    if (this.layoutButton) {
      this.layoutButton.onclick = () => this.toggleBottomLayout();
    }

    if (this.maximizeButton) {
      this.maximizeButton.onclick = () => this.toggleMaximize();
    }

    if (this.resizer) {
      const beginResize = (event) => {
        this.resizing = true;
        document.body.style.cursor = this.bottom ? 'row-resize' : 'col-resize';
        document.body.style.userSelect = 'none';
        if (event.target?.setPointerCapture && event.pointerId !== undefined) {
          try { event.target.setPointerCapture(event.pointerId); } catch {}
        }
        event.preventDefault();
      };
      const updateResize = (event) => {
        if (!this.resizing) return;
        if (this.bottom) {
          const statusBarHeight = document.querySelector('#status-bar')?.offsetHeight || 30;
          this.setHeight(window.innerHeight - event.clientY - statusBarHeight);
        } else {
          this.setWidth(window.innerWidth - event.clientX);
        }
      };
      const endResize = (event) => {
        if (!this.resizing) return;
        this.resizing = false;
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
        if (event?.target?.releasePointerCapture && event?.pointerId !== undefined) {
          try { event.target.releasePointerCapture(event.pointerId); } catch {}
        }
        this.onLayoutChange?.({ bottom: this.bottom, width: this.width, height: this.height });
      };

      this.resizer.addEventListener('pointerdown', beginResize);
      this.resizer.addEventListener('pointermove', updateResize);
      this.resizer.addEventListener('pointerup', endResize);
      this.resizer.addEventListener('pointercancel', endResize);
      document.addEventListener('pointermove', updateResize);
      document.addEventListener('pointerup', endResize);
      document.addEventListener('pointercancel', endResize);

      this.resizer.addEventListener('mousedown', beginResize);
      document.addEventListener('mousemove', updateResize);
      document.addEventListener('mouseup', endResize);

      this.resizer.addEventListener('keydown', (event) => {
        if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
        event.preventDefault();
        if (this.bottom) {
          const next = event.key === 'Home' ? 180 : event.key === 'End' ? 600 : this.height + (event.key === 'ArrowUp' ? 24 : -24);
          this.setHeight(next);
        } else {
          const next = event.key === 'Home' ? 240 : event.key === 'End' ? 620 : this.width + (event.key === 'ArrowLeft' ? 24 : -24);
          this.setWidth(next);
        }
      });
    }
  }

  _syncLayout() {
    const layout = document.querySelector('.layout');
    layout?.classList.toggle('preview-bottom', this.bottom);
    document.documentElement.style.setProperty('--preview-width', `${this.width}px`);
    document.documentElement.style.setProperty('--preview-height', `${this.height}px`);
    if (this.resizer) {
      this.resizer.setAttribute('aria-orientation', this.bottom ? 'horizontal' : 'vertical');
      this.resizer.setAttribute('aria-valuenow', String(this.bottom ? this.height : this.width));
    }
    if (this.layoutButton) {
      this.layoutButton.setAttribute('aria-pressed', String(this.bottom));
      const title = this.t(this.bottom ? 'movePreviewSide' : 'movePreviewBelow', this.bottom ? '移至侧边' : '移至下方');
      this.layoutButton.title = title;
      this.layoutButton.setAttribute('aria-label', title);
    }
  }

  setWidth(w) {
    this.width = Math.min(620, Math.max(240, Number(w) || 360));
    document.documentElement.style.setProperty('--preview-width', `${this.width}px`);
    if (this.resizer && !this.bottom) this.resizer.setAttribute('aria-valuenow', String(this.width));
  }

  setHeight(h) {
    this.height = Math.min(600, Math.max(180, Number(h) || 320));
    document.documentElement.style.setProperty('--preview-height', `${this.height}px`);
    if (this.resizer && this.bottom) this.resizer.setAttribute('aria-valuenow', String(this.height));
  }

  toggleBottomLayout() {
    this.bottom = !this.bottom;
    this._syncLayout();
    this.onLayoutChange?.({ bottom: this.bottom, width: this.width, height: this.height });
  }

  toggleMaximize() {
    if (!this.container) return;
    this.maximized = this.container.classList.toggle('is-maximized');
    if (this.maximizeButton) {
      this.maximizeButton.setAttribute('aria-pressed', String(this.maximized));
      const title = this.t(this.maximized ? 'previewRestore' : 'previewMaximize', this.maximized ? '还原预览' : '放大预览');
      this.maximizeButton.title = title;
      this.maximizeButton.setAttribute('aria-label', title);
    }
  }

  setOpen(open) {
    this.isOpen = Boolean(open);
    if (this.container) this.container.hidden = !this.isOpen;
    document.querySelector('.layout')?.classList.toggle('preview-open', this.isOpen);
  }

  open() {
    this.setOpen(true);
  }

  close() {
    this.setOpen(false);
    this.reset();
  }

  reset() {
    this._rememberEditorViewState();
    if (this.activePreviewId && this.call) {
      this.call('preview_cancel', { requestId: this.activePreviewId }).catch(() => {});
    }
    this.activePreviewId = undefined;
    if (this.previewObjectUrl) {
      URL.revokeObjectURL(this.previewObjectUrl);
      this.previewObjectUrl = undefined;
    }
    document.getElementById('image-lightbox')?.close();
    this.previewGeneration++;
    this.editorState = undefined;
    this.imageEditorState = undefined;
    if (this.body) {
      this.body.replaceChildren(Object.assign(document.createElement('p'), { className: 'muted', textContent: this.t('selectPreview', '选择文件查看预览') }));
    }
  }

  _rememberEditorViewState() {
    const editor = this.body?.querySelector('.file-editor');
    if (!this.editorState?.path || !editor) return;
    this.editorViewStates.set(this.editorState.path, {
      scrollTop: editor.scrollTop,
      selectionStart: editor.selectionStart,
      selectionEnd: editor.selectionEnd,
    });
    while (this.editorViewStates.size > 20) {
      this.editorViewStates.delete(this.editorViewStates.keys().next().value);
    }
  }

  async renderSelection(selectedItems = []) {
    this._rememberEditorViewState();
    const item = selectedItems[0];
    if (!item || item.isDir) {
      this.setOpen(false);
      this.reset();
      return;
    }

    this.open();
    const generation = ++this.previewGeneration;
    if (this.activePreviewId && this.call) {
      this.call('preview_cancel', { requestId: this.activePreviewId }).catch(() => {});
    }
    this.activePreviewId = undefined;

    if (item.kind === 'image') return this.renderImagePreview(item, generation);
    if (item.kind === 'archive') return this.renderArchivePreview(item, generation);
    if (item.kind === 'pdf') return this.renderPdfPreview(item, generation);
    if (item.kind === 'audio' || item.kind === 'video') return this.renderMediaPreview(item, generation);

    return this.renderTextPreview(item, generation);
  }

  _previewMeta(item) {
    const meta = document.createElement('p');
    meta.className = 'preview-meta muted';
    const fields = [
      [this.t('name', '名称'), item.name],
      [this.t('size', '大小'), this.formatSize(Number(item.size) || 0)],
      [this.t('createdAt', '创建'), Number(item.btime) ? new Date(item.btime).toLocaleString() : ''],
      [this.t('modifiedAt', '修改'), Number(item.mtime) ? new Date(item.mtime).toLocaleString() : ''],
      [this.t('path', '路径'), item.path],
    ];
    for (const [label, value] of fields) {
      if (!value) continue;
      const field = document.createElement('span');
      field.className = 'preview-meta-field';
      const key = document.createElement('strong');
      key.textContent = `${label}:`;
      const text = document.createElement('span');
      text.textContent = String(value);
      field.append(key, text);
      meta.append(field);
    }
    return meta;
  }

  async renderTextPreview(item, generation) {
    if (!this.body) return;
    this.body.replaceChildren(this._previewMeta(item));
    const loading = document.createElement('p');
    loading.className = 'muted';
    loading.textContent = this.t('loading', '加载中…');
    this.body.append(loading);

    const requestId = crypto.randomUUID();
    this.activePreviewId = requestId;

    try {
      const result = await this.call('read_file', { path: item.path }, requestId);
      if (generation !== this.previewGeneration || this.activePreviewId !== requestId) return;
      this.activePreviewId = undefined;
      loading.remove();

      const TEXT_KINDS = new Set(['text', 'json', 'code', 'markdown', 'csv']);
      if (!TEXT_KINDS.has(result.kind) && !/\.(txt|md|markdown|json|js|ts|html?|css|csv|py|rs|go|c|cpp|h|sh|yml|yaml|toml|xml|sql|log)$/i.test(item.name)) {
        this.renderUnsupportedPreview(item);
        return;
      }

      if (/\.html?$/i.test(item.name)) {
        this.editorState = undefined;
        this.renderHtmlPreview(item, result.content || '');
      } else {
        this.renderTextEditor(item, result);
        this.appendEditorActions(item);
      }
      this.onRememberPath?.(item.path);
    } catch (error) {
      if (generation !== this.previewGeneration) return;
      loading.remove();
      this.renderPreviewError(item, error.message === 'preview cancelled' ? this.t('previewCancelled', '预览已取消') : `${this.t('readFailed', '读取文件失败')}：${error.message}`);
    } finally {
      if (this.activePreviewId === requestId) this.activePreviewId = undefined;
    }
  }

  renderUnsupportedPreview(item) {
    if (!this.body) return;
    this.body.append(this._previewMeta(item));
    const note = document.createElement('p');
    note.className = 'muted';
    note.textContent = this.t('previewUnsupported', '此文件类型不在安全内置预览范围内。');
    this.body.append(note);
    const actions = document.createElement('div');
    actions.className = 'preview-actions';
    actions.append(
      this._button(this.t('open', '打开'), () => this.onOpenItem?.(item)),
      this._button(this.t('reveal', '显示'), () => this.onRevealItem?.(item.path))
    );
    this.body.append(actions);
  }

  renderPreviewError(item, message) {
    if (!this.body) return;
    const failed = document.createElement('p');
    failed.className = 'error';
    failed.textContent = message;
    this.body.append(failed);
    const actions = document.createElement('div');
    actions.className = 'preview-actions';
    actions.append(
      this._button(this.t('retry', '重试'), () => this.renderSelection([item])),
      this._button(this.t('open', '打开'), () => this.onOpenItem?.(item)),
      this._button(this.t('reveal', '显示'), () => this.onRevealItem?.(item.path))
    );
    this.body.append(actions);
  }

  _button(label, action, primary = false) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.textContent = label;
    if (primary) btn.className = 'primary';
    btn.onclick = action;
    return btn;
  }

  renderHtmlPreview(item, content) {
    if (!this.body) return;
    const meta = this._previewMeta(item);
    const frame = document.createElement('iframe');
    frame.className = 'html-preview';
    frame.setAttribute('sandbox', 'allow-same-origin');
    const policy = '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; img-src data: blob:; style-src \'unsafe-inline\';">';
    if (this.previewObjectUrl) URL.revokeObjectURL(this.previewObjectUrl);
    this.previewObjectUrl = URL.createObjectURL(new Blob([policy, String(content || '')], { type: 'text/html' }));
    frame.src = this.previewObjectUrl;
    this.body.replaceChildren(meta, frame);
    const actions = document.createElement('div');
    actions.className = 'preview-actions';
    actions.append(
      this._button(this.t('open', '打开'), () => this.onOpenItem?.(item)),
      this._button(this.t('reveal', '显示'), () => this.onRevealItem?.(item.path))
    );
    this.body.append(actions);
  }

  renderTextEditor(item, result) {
    if (!this.body) return;
    const readOnly = Boolean(result.truncated);
    this.editorState = {
      path: item.path,
      mtime: Number(result.mtime) || Number(item.mtime) || 0,
      savedContent: result.content || '',
      dirty: false,
      readOnly,
    };

    const meta = document.createElement('p');
    meta.className = 'preview-meta muted';
    meta.textContent = `${this.formatSize(result.size)} · ${result.encoding || 'utf-8'}${readOnly ? ` · ${this.t('previewTruncated', '内容已截断')}` : ''}`;

    const toolbar = document.createElement('div');
    toolbar.className = 'editor-toolbar';
    const stateLabel = document.createElement('span');
    stateLabel.className = 'muted';
    stateLabel.textContent = readOnly ? this.t('largeTextReadOnly', '大文件仅显示预览，编辑已禁用') : this.t('saved', '已保存');

    const saveBtn = document.createElement('button');
    saveBtn.textContent = this.t('save', '保存');
    saveBtn.disabled = true;
    saveBtn.hidden = readOnly;
    saveBtn.onclick = () => this.saveCurrentEditor();

    const isMd = /\.(md|markdown|mdx)$/i.test(item.name);
    const toggleBtn = document.createElement('button');
    toggleBtn.className = 'markdown-mode-toggle';
    toggleBtn.textContent = this.t('readMode', '阅读');
    toggleBtn.hidden = !isMd;

    toolbar.append(stateLabel, saveBtn, toggleBtn);

    const editor = document.createElement('textarea');
    editor.className = 'file-editor';
    editor.value = result.content || '';
    editor.readOnly = readOnly;
    editor.spellcheck = false;
    editor.setAttribute('aria-label', this.t('editorLabel', '文件编辑器'));

    editor.addEventListener('input', () => {
      if (!this.editorState || this.editorState.readOnly) return;
      this.editorState.dirty = editor.value !== this.editorState.savedContent;
      stateLabel.textContent = this.editorState.dirty ? this.t('unsaved', '未保存') : this.t('saved', '已保存');
      saveBtn.disabled = !this.editorState.dirty;
    });

    editor.addEventListener('keydown', (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        this.saveCurrentEditor();
      }
    });

    toggleBtn.onclick = () => {
      const reading = toggleBtn.dataset.mode !== 'reading';
      toggleBtn.dataset.mode = reading ? 'reading' : 'source';
      toggleBtn.textContent = this.t(reading ? 'sourceMode' : 'readMode', reading ? '源码' : '阅读');
      editor.hidden = reading;
      this.body.querySelectorAll('.markdown-preview').forEach((node) => { node.hidden = !reading; });
    };

    this.body.replaceChildren(meta, toolbar, editor);
  }

  appendEditorActions(item) {
    if (!this.body) return;
    const actions = document.createElement('div');
    actions.className = 'preview-actions';
    actions.append(
      this._button(this.t('open', '打开'), () => this.onOpenItem?.(item)),
      this._button(this.t('reveal', '显示'), () => this.onRevealItem?.(item.path))
    );
    this.body.append(actions);
  }

  async saveCurrentEditor() {
    if (!this.editorState || this.editorState.readOnly || !this.editorState.dirty) return true;
    const editor = this.body?.querySelector('.file-editor');
    if (!editor) return false;
    const content = editor.value;
    const path = this.editorState.path;
    const parts = path.split('/');
    const name = parts.pop() || '';
    const parent = parts.join('/') || '/';

    try {
      const data = btoa(unescape(encodeURIComponent(content)));
      await this.call('write_file', { parent, name, data });
      this.editorState.savedContent = content;
      this.editorState.dirty = false;
      const saveBtn = this.body?.querySelector('.editor-toolbar button');
      if (saveBtn) saveBtn.disabled = true;
      const stateLabel = this.body?.querySelector('.editor-toolbar .muted');
      if (stateLabel) stateLabel.textContent = this.t('saved', '已保存');
      this.onToast?.(this.t('saved', '已保存'), 'success');
      this.onDirectoryReload?.();
      return true;
    } catch (error) {
      this.onStatus?.(`${this.t('saveFailed', '保存失败')}：${error.message}`, 'error');
      return false;
    }
  }

  async renderImagePreview(item, generation) {
    if (!this.body) return;
    this.body.replaceChildren(this._previewMeta(item));
    const loading = document.createElement('p');
    loading.className = 'muted';
    loading.textContent = this.t('loading', '加载中…');
    this.body.append(loading);

    const requestId = crypto.randomUUID();
    this.activePreviewId = requestId;

    try {
      const result = await this.call('image_preview', { path: item.path }, requestId);
      if (generation !== this.previewGeneration || this.activePreviewId !== requestId) return;
      loading.remove();
      const img = document.createElement('img');
      img.className = 'preview-image';
      img.src = `data:${result.mimeType};base64,${result.data}`;
      img.alt = item.name;
      this.body.append(img);
      this.appendEditorActions(item);
    } catch (error) {
      if (generation !== this.previewGeneration) return;
      loading.remove();
      this.renderPreviewError(item, error.message);
    }
  }

  async renderArchivePreview(item, generation) {
    if (!this.body) return;
    this.body.replaceChildren(this._previewMeta(item));
    const loading = document.createElement('p');
    loading.className = 'muted';
    loading.textContent = this.t('loading', '加载中…');
    this.body.append(loading);

    const requestId = crypto.randomUUID();
    this.activePreviewId = requestId;

    try {
      const result = await this.call('archive_list', { path: item.path }, requestId);
      if (generation !== this.previewGeneration || this.activePreviewId !== requestId) return;
      loading.remove();
      const list = document.createElement('ul');
      list.className = 'archive-entries';
      for (const entry of result.entries || []) {
        const row = document.createElement('li');
        row.className = 'archive-entry';
        row.textContent = `${entry.name} (${this.formatSize(Number(entry.size) || 0)})`;
        list.append(row);
      }
      this.body.append(list);
      this.appendEditorActions(item);
    } catch (error) {
      if (generation !== this.previewGeneration) return;
      loading.remove();
      this.renderPreviewError(item, error.message);
    }
  }

  async renderPdfPreview(item, generation) {
    if (!this.body) return;
    this.body.replaceChildren(this._previewMeta(item));
    const loading = document.createElement('p');
    loading.className = 'muted';
    loading.textContent = this.t('loading', '加载中…');
    this.body.append(loading);

    const requestId = crypto.randomUUID();
    this.activePreviewId = requestId;

    try {
      const result = await this.call('pdf_preview', { path: item.path }, requestId);
      if (generation !== this.previewGeneration || this.activePreviewId !== requestId) return;
      loading.remove();
      const frame = document.createElement('iframe');
      frame.className = 'pdf-preview';
      const bytes = Uint8Array.from(atob(result.data || ''), (c) => c.charCodeAt(0));
      if (this.previewObjectUrl) URL.revokeObjectURL(this.previewObjectUrl);
      this.previewObjectUrl = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));
      frame.src = `${this.previewObjectUrl}#page=1&zoom=page-width`;
      this.body.append(frame);
      this.appendEditorActions(item);
    } catch (error) {
      if (generation !== this.previewGeneration) return;
      loading.remove();
      this.renderPreviewError(item, error.message);
    }
  }

  async renderMediaPreview(item, generation) {
    if (!this.body) return;
    this.body.replaceChildren(this._previewMeta(item));
    const note = document.createElement('p');
    note.className = 'muted';
    note.textContent = this.t('mediaPreviewUnavailable', '音视频使用系统默认应用打开');
    this.body.append(note);
    this.appendEditorActions(item);
  }
}

export function createFilesPreviewPanel(options) {
  return new FilesPreviewPanel(options);
}
