/**
 * FilesHomeView Component
 * Renders the default empty home workspace when no directory is currently selected.
 */
export class FilesHomeView {
  constructor({ container, onNavigate, onSearch, t = (k, f) => f || k }) {
    this.container = typeof container === 'string' ? document.querySelector(container) : container;
    this.onNavigate = onNavigate;
    this.onSearch = onSearch;
    this.t = t;
  }

  render() {
    if (!this.container) return;
    this.container.replaceChildren();

    const wrap = document.createElement('div');
    wrap.className = 'home-welcome-view';

    const icon = document.createElement('div');
    icon.className = 'home-welcome-icon';
    icon.innerHTML = `<svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.3 7 12 12 20.7 7"/><line x1="12" y1="22" x2="12" y2="12"/></svg>`;

    const title = document.createElement('h2');
    title.className = 'home-welcome-title';
    title.textContent = this.t('welcomeTitle', '请从左侧选择目录');

    const desc = document.createElement('p');
    desc.className = 'home-welcome-desc muted';
    desc.textContent = this.t('welcomeDesc', '从左侧侧边栏选择工作目录，或按 ⌘K 快速全局搜索文件。');

    const shortcuts = document.createElement('div');
    shortcuts.className = 'home-welcome-shortcuts';

    const searchBtn = document.createElement('button');
    searchBtn.type = 'button';
    searchBtn.className = 'home-welcome-btn primary';
    searchBtn.innerHTML = `<svg class="icon"><use href="#i-search"/></svg><span>${this.t('searchFiles', '搜索文件')} (⌘K)</span>`;
    searchBtn.onclick = () => this.onSearch?.();

    shortcuts.append(searchBtn);

    wrap.append(icon, title, desc, shortcuts);
    this.container.append(wrap);
  }
}

export function createFilesHomeView(options) {
  return new FilesHomeView(options);
}
