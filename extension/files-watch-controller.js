export function createFilesWatchController({
  $, session, t, parentAndName, editorState, imageEditorState, hasDirtyEditor,
  resetPreview, getBatch, updateProgress, setStatus, renderEntries,
  getSearchQuery, search, loadDirectory, renderPreviewSelection,
  refreshEditorAfterExternalChange, renderSelection, storageSet,
}) {
  const changedPaths = new Map();
  const selfOpenedPaths = new Map();
  let watchedPath;
  let reloadTimer;
  let changedCleanupTimer;
  let followApplyTimer;
  let followAppliedAt = 0;
  let pendingFollowPath;

  function resetWatchedPath() { watchedPath = undefined; }
  function setWatchedPath(path) { watchedPath = path; }
  function clearPendingFollow() {
    pendingFollowPath = undefined;
    clearTimeout(followApplyTimer);
    followApplyTimer = undefined;
  }

  function isSelfOpened(path) {
    const timestamp = selfOpenedPaths.get(path);
    if (!timestamp) return false;
    if (Date.now() - timestamp < 3_000) return true;
    selfOpenedPaths.delete(path);
    return false;
  }

  function markSelfOpened(path) {
    if (!path) return;
    selfOpenedPaths.set(path, Date.now());
    setTimeout(() => {
      const timestamp = selfOpenedPaths.get(path);
      if (timestamp && Date.now() - timestamp >= 3_000) selfOpenedPaths.delete(path);
    }, 3_100);
  }

  function markChangedPath(path, kind = 'modified') {
    if (!path) return;
    if (session.followChanges && !editorState()?.dirty && parentAndName(path).parent === session.currentPath) pendingFollowPath = path;
    const previous = changedPaths.get(path);
    changedPaths.set(path, { timestamp: Date.now(), count: (previous?.count || 0) + 1, kind });
    if (changedPaths.size > 256) changedPaths.delete(changedPaths.keys().next().value);
    clearTimeout(changedCleanupTimer);
    changedCleanupTimer = setTimeout(() => {
      const cutoff = Date.now() - 3_000;
      for (const [changedPath, change] of changedPaths) if (change.timestamp < cutoff) changedPaths.delete(changedPath);
      renderEntries();
    }, 3_050);
  }

  function handleNativeMessage(message) {
    const messageId = typeof message?.id === 'string' ? message.id : '';
    if ((message?.result?.event === 'batch_progress' || message?.result?.event === 'archive_progress') && getBatch()?.requestId === messageId.replace(/:progress$/, '')) {
      const completed = Number(message.result.completed) || 0;
      const total = Number(message.result.total) || 1;
      updateProgress(completed, total);
      const phase = message.result.event === 'archive_progress'
        ? (message.result.phase === 'validating' ? t('archiveValidating', '校验中') : t('archiveCompressing', '压缩中'))
        : t('processing', '处理中');
      setStatus(`${phase} · ${completed}/${total}`);
      return;
    }
    if (message?.result?.event !== 'fs_changed' || !session.currentPath) return;
    const changedPath = message.result.path;
    const name = String(changedPath || '').split('/').pop() || '';
    if (/\.(swp|tmp|part|lock)$/i.test(name) || /(?:-journal|-shm|-wal)$/i.test(name) || isSelfOpened(changedPath)) return;
    if (message.result.kind === 'removed' && (editorState()?.path === changedPath || imageEditorState()?.path === changedPath) && !hasDirtyEditor()) resetPreview();
    markChangedPath(changedPath, message.result.kind || 'modified');
    const watchedDirectory = session.currentPath;
    clearTimeout(reloadTimer);
    reloadTimer = setTimeout(() => {
      reloadTimer = undefined;
      if (session.currentPath !== watchedDirectory) return;
      const scrollTop = document.querySelector('.content')?.scrollTop || 0;
      const searchQuery = getSearchQuery();
      const refresh = searchQuery ? search(searchQuery) : loadDirectory(watchedDirectory);
      Promise.resolve(refresh).then(() => {
        if (session.currentPath === watchedDirectory) {
          const viewport = document.querySelector('.content');
          if (viewport) viewport.scrollTop = scrollTop;
        }
      });
      if (session.selectedPaths.has(changedPath) && !editorState()?.dirty) renderPreviewSelection();
      if (editorState()?.path === changedPath) refreshEditorAfterExternalChange(changedPath);
    }, 250);
  }

  function handleNativeDisconnect(error, wasIntentional) {
    resetWatchedPath();
    if (!hasDirtyEditor()) {
      resetPreview();
      window.__nativesPreviewCleanup = true;
    }
    if (!wasIntentional) {
      if ($('retry')) $('retry').hidden = false;
      setStatus(`${t('hostDisconnected', 'Native Host 已断开')}：${error.message}`, 'error');
    }
  }

  function stopFollowOnManual() {
    if (!session.followChanges) return;
    session.followChanges = false;
    clearPendingFollow();
    if ($('follow-changes')) $('follow-changes').checked = false;
    storageSet('natives-follow-changes', false).catch(() => {});
    setStatus(t('followChangesStopped', '手动浏览，已停止跟随'));
  }

  async function applyPendingFollow(path) {
    const target = pendingFollowPath;
    if (!session.followChanges || !target || editorState()?.dirty || parentAndName(target).parent !== path) return;
    const wait = followAppliedAt ? Math.max(0, 900 - (Date.now() - followAppliedAt)) : 0;
    if (wait) {
      clearTimeout(followApplyTimer);
      followApplyTimer = setTimeout(() => { followApplyTimer = undefined; applyPendingFollow(path); }, wait);
      return;
    }
    const item = session.entries.find((entry) => entry.path === target);
    if (!item || item.isDir) return;
    pendingFollowPath = undefined;
    followAppliedAt = Date.now();
    session.selectedPaths = new Set([target]);
    session.lastSelectedIndex = session.entries.indexOf(item);
    renderSelection();
    document.querySelector(`[data-path="${CSS.escape(target)}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  return {
    changedPaths, handleNativeMessage, handleNativeDisconnect, markSelfOpened,
    stopFollowOnManual, clearPendingFollow, applyPendingFollow,
    resetWatchedPath, setWatchedPath, get watchedPath() { return watchedPath; },
  };
}
