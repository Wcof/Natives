import { createSidebarController } from './sidebar-controller.js';
import { createSettingsMenu } from './settings-menu.js';

/**
 * FilesSidebar Component
 * Manages the left product navigation sidebar: branding, search trigger,
 * extensible directory menu sections/roots, preferences, and responsive resize/collapse.
 */
export class FilesSidebar {
  constructor({
    container,
    resizer,
    toggleButton,
    initialWidth = 248,
    initialCollapsed = false,
    roots = [],
    sections = [],
    onNavigate,
    onSearch,
    onWidthChange,
    onCollapsedChange,
    onLanguageChange,
    onThemeChange,
    onModelSettings,
    t = (key, fallback) => fallback || key,
  }) {
    this.container = typeof container === 'string' ? document.querySelector(container) : container;
    this.resizer = typeof resizer === 'string' ? document.querySelector(resizer) : resizer;
    this.toggleButton = typeof toggleButton === 'string' ? document.querySelector(toggleButton) : toggleButton;
    this.width = Math.min(420, Math.max(190, Number(initialWidth) || 248));
    this.collapsed = Boolean(initialCollapsed);
    this.roots = roots;
    this.sections = sections;
    this.activePath = '';
    this.onNavigate = onNavigate;
    this.onSearch = onSearch;
    this.onWidthChange = onWidthChange;
    this.onCollapsedChange = onCollapsedChange;
    this.onLanguageChange = onLanguageChange;
    this.onThemeChange = onThemeChange;
    this.onModelSettings = onModelSettings;
    this.t = t;

    this.language = 'zh_CN';
    this.theme = 'archive';
    this._init();
  }

  _init() {
    this.sidebarController = createSidebarController({
      resizer: this.resizer,
      toggleButton: this.toggleButton,
      initialWidth: this.width,
      initialCollapsed: this.collapsed,
      t: this.t,
      onWidthChange: (width) => { this.width = width; this.onWidthChange?.(width); },
      onCollapsedChange: (collapsed) => { this.collapsed = collapsed; this.onCollapsedChange?.(collapsed); },
    });
    this._bindEvents();
    if (this.roots.length > 0 || this.sections.length > 0) {
      this.render();
    }
  }

  _bindEvents() {
    const searchTrigger = this.container?.querySelector('#command-search-trigger');
    if (searchTrigger) {
      searchTrigger.onclick = () => this.onSearch?.();
    }

    const settingsEntry = this.container?.querySelector('#settings-entry');
    if (settingsEntry) {
      this.settingsEntry = settingsEntry;
      this.settingsMenu = createSettingsMenu({
        anchorButton: settingsEntry,
        initialLanguage: this.language,
        initialTheme: this.theme,
        onLanguageChange: (lang) => {
          this.language = lang;
          this.onLanguageChange?.(lang);
        },
        onThemeChange: (theme) => {
          this.theme = theme;
          this.onThemeChange?.(theme);
        },
        onModelSettings: (anchor) => this.onModelSettings?.(anchor),
        t: this.t,
      });
      settingsEntry.onclick = () => this.settingsMenu?.toggle();
    }

    this.container?.addEventListener('click', (event) => {
      const itemBtn = event.target.closest('.app-menu-item');
      if (!itemBtn || itemBtn.disabled) return;
      const rootId = itemBtn.dataset.rootId;
      const targetPath = itemBtn.dataset.path;
      if (targetPath) {
        this.setActivePath(targetPath);
        this.onNavigate?.(targetPath, { id: rootId, path: targetPath });
      } else if (rootId) {
        const root = this.roots.find((r) => r.id === rootId);
        if (root) {
          this.setActivePath(root.path);
          this.onNavigate?.(root.path, root);
        }
      }
    });
  }

  setWidth(width, notify = true) {
    this.sidebarController.setWidth(width, notify);
    this.width = this.sidebarController.width;
  }

  setCollapsed(collapsed, notify = true) {
    this.sidebarController.setCollapsed(collapsed, notify);
    this.collapsed = this.sidebarController.collapsed;
  }

  toggle() {
    this.sidebarController.toggle();
    this.collapsed = this.sidebarController.collapsed;
  }

  setRoots(roots = []) {
    this.roots = roots;
    this.render();
  }

  setSections(sections = []) {
    this.sections = sections;
    this.render();
  }

  addSection(section) {
    this.sections.push(section);
    this.render();
  }

  setActivePath(path) {
    this.activePath = path;
    const items = this.container?.querySelectorAll('.app-menu-item') || [];
    items.forEach((item) => {
      const root = this.roots.find((r) => r.id === item.dataset.rootId);
      const isMatch = (item.dataset.path && item.dataset.path === path) || (root && root.path === path);
      item.classList.toggle('active', Boolean(isMatch));
      item.setAttribute('aria-current', isMatch ? 'page' : 'false');
    });
  }

  render() {
    const menuNav = this.container?.querySelector('.app-menu');
    if (!menuNav) return;

    menuNav.replaceChildren();

    const docSection = document.createElement('div');
    docSection.className = 'app-menu-section';
    const sectionTitle = document.createElement('span');
    sectionTitle.setAttribute('data-i18n', 'navDocuments');
    sectionTitle.textContent = this.t('navDocuments', '文档');
    docSection.append(sectionTitle);

    const defaultItems = [
      { id: 'desktop', icon: 'panel', i18n: 'desktop', fallback: '桌面' },
      { id: 'downloads', icon: 'download', i18n: 'downloads', fallback: '下载' },
      { id: 'documents', icon: 'file', i18n: 'documents', fallback: '文档' },
    ];

    defaultItems.forEach(({ id, icon, i18n, fallback }) => {
      const root = this.roots.find((r) => r.id === id);
      const btn = document.createElement('button');
      btn.className = 'app-menu-item';
      btn.type = 'button';
      btn.dataset.rootId = id;
      if (root) btn.dataset.path = root.path;
      btn.disabled = !root && this.roots.length > 0;
      btn.innerHTML = `<svg class="icon"><use href="#i-${icon}" /></svg><span data-i18n="${i18n}">${this.t(i18n, fallback)}</span>`;
      docSection.append(btn);
    });

    menuNav.append(docSection);

    for (const section of this.sections) {
      const secDiv = document.createElement('div');
      secDiv.className = 'app-menu-section';
      if (section.title) {
        const titleSpan = document.createElement('span');
        if (section.i18n) titleSpan.setAttribute('data-i18n', section.i18n);
        titleSpan.textContent = section.title;
        secDiv.append(titleSpan);
      }
      for (const item of section.items || []) {
        const btn = document.createElement('button');
        btn.className = 'app-menu-item';
        btn.type = 'button';
        if (item.id) btn.dataset.itemId = item.id;
        if (item.path) btn.dataset.path = item.path;
        btn.innerHTML = `<svg class="icon"><use href="#i-${item.icon || 'folder'}" /></svg><span>${item.label}</span>`;
        if (item.badge) {
          const badge = document.createElement('span');
          badge.className = 'menu-badge';
          badge.textContent = item.badge;
          btn.append(badge);
        }
        secDiv.append(btn);
      }
      menuNav.append(secDiv);
    }

    if (this.activePath) {
      this.setActivePath(this.activePath);
    }
  }

  setPreferences({ language, theme } = {}) {
    if (language) this.language = language;
    if (theme) this.theme = theme;
    this.sidebarController.refreshLabel();
    this.settingsMenu?.setPreferences({ language, theme });
  }

  toggleSettings(force, focusSubmenu) {
    this.settingsMenu?.toggle(force, focusSubmenu);
  }

  openSettings(focusSubmenu) {
    this.settingsMenu?.open(focusSubmenu);
  }
}

export function createFilesSidebar(options) {
  return new FilesSidebar(options);
}
