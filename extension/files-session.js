export function createFilesSession() {
  return {
    currentPath: undefined,
    entries: [],
    selectedPaths: new Set(),
    lastSelectedIndex: -1,
    history: [],
    historyIndex: -1,
    pageOffset: 0,
    pageHasMore: false,

    
    viewMode: 'list',
    gridSize: 'medium',
    sortBy: 'name',
    sortDirection: 'asc',
    showHidden: false,
    followChanges: false,
    sidebarWidth: 248,
    sidebarCollapsed: false,
    previewWidth: 360,
    previewHeight: 320,
    previewBottom: false,
    
    activePreviewId: undefined,
    previewGeneration: 0,
    previewObjectUrl: undefined,

    navigate(path, push = true) {
      if (push && this.currentPath && this.currentPath !== path) {
        this.history = this.history.slice(0, this.historyIndex + 1);
        this.history.push(path);
        this.historyIndex++;
      }
      if (this.historyIndex < 0) {
        this.history = [path];
        this.historyIndex = 0;
      }
      this.currentPath = path;
      this.pageOffset = 0;
      this.pageHasMore = false;
      this.selectedPaths.clear();
      this.lastSelectedIndex = -1;
    },

    moveHistory(delta) {
      const index = this.historyIndex + delta;
      if (index < 0 || index >= this.history.length) return undefined;
      this.historyIndex = index;
      return this.history[index];
    },

    select(index, modifiers = {}) {
      const item = this.entries[index];
      if (!item) return;
      if (modifiers.shiftKey && this.lastSelectedIndex >= 0) {
        const [start, end] = [this.lastSelectedIndex, index].sort((a, b) => a - b);
        this.selectedPaths = new Set(this.entries.slice(start, end + 1).map((entry) => entry.path));
      } else if (modifiers.metaKey || modifiers.ctrlKey) {
        this.selectedPaths.has(item.path) ? this.selectedPaths.delete(item.path) : this.selectedPaths.add(item.path);
        this.lastSelectedIndex = index;
      } else {
        this.selectedPaths = new Set([item.path]);
        this.lastSelectedIndex = index;
      }
    },
  };
}
