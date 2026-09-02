export function bindFilesPreviewLayout({ $, session, storageSet, t }) {
  let resizing = false;

  function syncControls() {
    const bottom = session.previewBottom;
    const button = $('toggle-preview-layout');
    if (button) {
      button.title = bottom ? t('movePreviewSide', 'Move preview to the side') : t('movePreviewBelow', 'Move preview below');
      button.setAttribute('aria-label', button.title);
      button.setAttribute('aria-pressed', String(bottom));
    }
    const resizer = $('preview-resizer');
    if (!resizer) return;
    resizer.setAttribute('aria-orientation', bottom ? 'horizontal' : 'vertical');
    resizer.setAttribute('aria-valuenow', String(bottom ? session.previewHeight : session.previewWidth));
    resizer.setAttribute('aria-valuemin', String(bottom ? 180 : 240));
    resizer.setAttribute('aria-valuemax', String(bottom ? 600 : 620));
  }

  function beginResize(event) {
    if ($('preview')?.classList.contains('is-maximized')) return;
    resizing = true;
    document.body.style.cursor = session.previewBottom ? 'row-resize' : 'col-resize';
    document.body.style.userSelect = 'none';
    if (event.target?.setPointerCapture && event.pointerId !== undefined) {
      try { event.target.setPointerCapture(event.pointerId); } catch {}
    }
    event.preventDefault();
  }

  function updateResize(event) {
    if (!resizing) return;
    if (session.previewBottom) {
      const statusBarHeight = $('status-bar')?.offsetHeight || 30;
      session.previewHeight = Math.min(600, Math.max(180, innerHeight - event.clientY - statusBarHeight));
    } else {
      session.previewWidth = Math.min(620, Math.max(240, innerWidth - event.clientX));
    }
    const value = session.previewBottom ? session.previewHeight : session.previewWidth;
    document.documentElement.style.setProperty(session.previewBottom ? '--preview-height' : '--preview-width', `${value}px`);
    $('preview-resizer')?.setAttribute('aria-valuenow', String(value));
  }

  function endResize(event) {
    if (!resizing) return;
    resizing = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    if (event?.target?.releasePointerCapture && event?.pointerId !== undefined) {
      try { event.target.releasePointerCapture(event.pointerId); } catch {}
    }
    const bottom = session.previewBottom;
    const value = bottom ? session.previewHeight : session.previewWidth;
    $('preview-resizer')?.setAttribute('aria-valuenow', String(value));
    storageSet(bottom ? 'natives-preview-height' : 'natives-preview-width', value).catch(() => {});
  }

  $('maximize-preview').onclick = () => {
    const maximized = $('preview')?.classList.toggle('is-maximized');
    const button = $('maximize-preview');
    if (!button) return;
    button.setAttribute('aria-pressed', String(maximized));
    button.title = t(maximized ? 'previewRestore' : 'previewMaximize', maximized ? '还原预览' : '放大预览');
    button.setAttribute('aria-label', button.title);
  };
  document.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' || !$('preview')?.classList.contains('is-maximized') || $('modal')?.open || $('dirty-modal')?.open || $('conflict-modal')?.open || $('usage-modal')?.open) return;
    event.preventDefault();
    $('maximize-preview')?.click();
  }, true);

  $('toggle-preview-layout').onclick = () => {
    session.previewBottom = !session.previewBottom;
    document.querySelector('.layout')?.classList.toggle('preview-bottom', session.previewBottom);
    syncControls();
    storageSet('natives-preview-bottom', session.previewBottom).catch(() => {});
  };

  const resizer = $('preview-resizer');
  if (resizer) {
    for (const type of ['pointerdown', 'mousedown']) resizer.addEventListener(type, beginResize);
    for (const type of ['pointermove']) resizer.addEventListener(type, updateResize);
    for (const type of ['pointerup', 'pointercancel']) resizer.addEventListener(type, endResize);
    resizer.addEventListener('keydown', (event) => {
      if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key) || $('preview')?.classList.contains('is-maximized')) return;
      const delta = event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -20 : event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 20 : 0;
      if (session.previewBottom) session.previewHeight = event.key === 'Home' ? 180 : event.key === 'End' ? 600 : Math.min(600, Math.max(180, session.previewHeight + delta));
      else session.previewWidth = event.key === 'Home' ? 240 : event.key === 'End' ? 620 : Math.min(620, Math.max(240, session.previewWidth + delta));
      const value = session.previewBottom ? session.previewHeight : session.previewWidth;
      document.documentElement.style.setProperty(session.previewBottom ? '--preview-height' : '--preview-width', `${value}px`);
      resizer.setAttribute('aria-valuenow', String(value));
      storageSet(session.previewBottom ? 'natives-preview-height' : 'natives-preview-width', value).catch(() => {});
      event.preventDefault();
    });
  }
  for (const type of ['pointermove', 'mousemove']) document.addEventListener(type, updateResize);
  for (const type of ['pointerup', 'pointercancel', 'mouseup']) document.addEventListener(type, endResize);
  $('preview')?.addEventListener('pointerdown', (event) => {
    if (event.target === resizer) return;
    const bounds = $('preview')?.getBoundingClientRect();
    if (!bounds) return;
    const edgeDistance = session.previewBottom ? event.clientY - bounds.top : event.clientX - bounds.left;
    if (edgeDistance <= 9) beginResize(event);
  });

  syncControls();
  return { syncControls };
}
