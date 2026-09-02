export function bindFilesEditorInteractions({ $, ops, contextMenu, iconElement, setStatus, t }) {
  document.addEventListener('paste', (event) => {
    const editor = event.target.closest('.file-editor');
    const file = [...(event.clipboardData?.files || [])].find((candidate) => /^image\//i.test(candidate.type || ''));
    if (!editor || !file) return;
    event.preventDefault();
    ops.importImageIntoEditor(file, editor).catch((error) => setStatus(error.message, 'error'));
  });

  document.addEventListener('drop', async (event) => {
    const editor = event.target.closest('.file-editor');
    const files = [...(event.dataTransfer?.files || [])].filter((candidate) => /^image\//i.test(candidate.type || '')).slice(0, 20);
    const html = event.dataTransfer?.getData('text/html') || '';
    const src = html.match(/<img[^>]+src=["']([^"']+)["']/i)?.[1];
    if (!editor || (!files.length && !src)) return;
    event.preventDefault();
    if (files.length) {
      for (const file of files) if (!await ops.importImageIntoEditor(file, editor)) break;
    } else if (/^file:/i.test(src)) {
      try {
        const url = new URL(src);
        if (url.hostname && url.hostname !== 'localhost') throw new Error(t('invalidPath', '路径无效'));
        ops.copyImageIntoEditor(decodeURIComponent(url.pathname), editor);
      } catch (error) {
        setStatus(error.message, 'error');
      }
    } else if (/^(https?:|data:image\/)/i.test(src)) {
      fetch(src)
        .then((response) => {
          if (!response.ok) throw new Error(t('imagePreviewUnavailable', '图片读取失败'));
          return response.blob();
        })
        .then((blob) => ops.importImageIntoEditor(new File([blob], `image-${Date.now()}.png`, { type: blob.type }), editor))
        .catch((error) => setStatus(error.message, 'error'));
    }
  }, true);

  function applyMarkdownFormat(action) {
    const editor = $('preview-body').querySelector('.file-editor');
    if (!editor) return;
    const start = editor.selectionStart;
    const end = editor.selectionEnd;
    const selected = editor.value.slice(start, end) || t('selectedText', 'text');
    const wrappers = { bold: ['**', '**'], italic: ['_', '_'], code: ['`', '`'], list: ['- ', ''], heading: ['# ', ''] };
    let [prefix, suffix] = wrappers[action] || wrappers.bold;
    if (action === 'link') {
      contextMenu.openModal({
        title: t('insertLink', '插入链接'),
        label: t('linkPrompt', '链接地址 (URL)'),
        value: 'https://',
        submit: (url) => {
          if (!url || !/^https?:\/\//i.test(url.trim())) return;
          prefix = '[';
          suffix = `](${url.trim()})`;
          editor.setRangeText(`${prefix}${selected}${suffix}`, start, end, 'end');
          editor.dispatchEvent(new Event('input', { bubbles: true }));
          editor.focus();
        },
      });
      return;
    }
    editor.setRangeText(`${prefix}${selected}${suffix}`, start, end, 'end');
    editor.dispatchEvent(new Event('input', { bubbles: true }));
    editor.focus();
  }

  new MutationObserver(() => {
    const left = $('preview-body').querySelector('.editor-toolbar-left');
    const toolbar = left?.closest('.editor-toolbar');
    if (!toolbar || !left || toolbar.dataset.formatReady) return;
    toolbar.dataset.formatReady = 'true';
    const tools = document.createElement('span');
    tools.className = 'markdown-tools';
    for (const action of ['bold', 'italic', 'code', 'list', 'heading', 'link', 'image']) {
      const button = document.createElement('button');
      button.type = 'button';
      button.dataset.mdAction = action;
      button.append(iconElement(action));
      button.title = t(`markdown${action[0].toUpperCase()}${action.slice(1)}`, action);
      button.setAttribute('aria-label', button.title);
      tools.append(button);
    }
    left.prepend(tools);
  }).observe($('preview-body'), { childList: true, subtree: true });

  document.addEventListener('click', (event) => {
    const button = event.target.closest('[data-md-action]');
    if (!button) return;
    if (button.dataset.mdAction === 'image') {
      const input = document.createElement('input');
      input.type = 'file';
      input.multiple = true;
      input.accept = 'image/*';
      input.onchange = async () => {
        const editor = $('preview-body').querySelector('.file-editor');
        if (!editor) return;
        for (const file of input.files || []) if (!await ops.importImageIntoEditor(file, editor)) break;
      };
      input.click();
    } else {
      applyMarkdownFormat(button.dataset.mdAction);
    }
  });

}
