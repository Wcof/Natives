export function createFilesFeedback({ $, session, t, selectedItems, getClipboard, isHostConnected, formatSize }) {
  function setStatus(message, kind = '') {
    const element = $('status');
    if (!element) return;
    element.textContent = message;
    element.className = kind;
  }

  function renderStatusBar() {
    const selectedSize = selectedItems().reduce((total, item) => total + (item.isDir ? 0 : Number(item.size) || 0), 0);
    const clipboard = getClipboard();
    const selection = $('selection-status');
    if (selection) {
      selection.textContent = `${session.entries.length} ${t('items', '个项目')} · ${session.selectedPaths.size} ${t('selected', '已选择')} · ${t('selectedSize', '选中大小')} ${formatSize(selectedSize)}`;
    }
    const clipboardStatus = $('file-clipboard-status');
    if (clipboardStatus) {
      clipboardStatus.textContent = clipboard
        ? `${clipboard.mode === 'copy' ? t('clipboardCopied', '已复制') : t('clipboardCut', '已剪切')} ${clipboard.paths.length} ${t('items', '项')}`
        : '';
    }
    const clear = $('clear-file-clipboard');
    if (clear) clear.hidden = !clipboard;
    const hostStatus = $('host-status');
    if (hostStatus) hostStatus.textContent = isHostConnected() ? t('hostConnected', 'Host 已连接') : t('hostDisconnectedShort', 'Host 未连接');
  }

  function updateProgress(value, max, visible = true) {
    const progress = $('operation-progress');
    if (!progress) return;
    progress.max = Math.max(1, max || 1);
    progress.value = Math.min(progress.max, Math.max(0, value || 0));
    progress.hidden = !visible;
  }

  function setOperationCancelable(visible) {
    const button = $('cancel-operation');
    if (button) button.hidden = !visible;
  }

  function toast(message, kind = '') {
    const element = $('toast');
    if (!element) return;
    element.textContent = message;
    element.className = kind;
    element.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => { element.hidden = true; }, 3_200);
  }

  return { setStatus, renderStatusBar, updateProgress, setOperationCancelable, toast };
}
