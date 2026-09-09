// 统一空间写队列：串行执行 workspace 写操作，每次使用该空间的最新 revision；
// 迟到的响应只更新对应空间，避免切换空间后覆盖当前画面。
export function createWorkspaceMutationQueue({
  nativeCall,
  getActiveWorkspaceId,
  getActiveSnapshot,
  applySnapshot,
}) {
  let chain = Promise.resolve();
  return function queueWorkspaceMutation(workspaceId, task) {
    const run = chain.then(async () => {
      let snapshot;
      if (workspaceId === getActiveWorkspaceId() && getActiveSnapshot()) {
        snapshot = getActiveSnapshot();
      } else {
        snapshot = await nativeCall('workspace_snapshot', { workspaceId });
      }
      const result = await task(snapshot);
      if (workspaceId === getActiveWorkspaceId() && result) {
        applySnapshot(result);
      }
      return result;
    });
    chain = run.catch(() => {});
    return run;
  };
}
