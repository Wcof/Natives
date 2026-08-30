/**
 * Shared App Settings Menu (Language & Theme preferences) (<160 lines).
 * Reused by both files.html and space.html.
 */

export function createSettingsMenu({
  anchorButton,
  initialLanguage = 'zh_CN',
  initialTheme = 'archive',
  onLanguageChange,
  onThemeChange,
  t = (k, f) => f || k,
}) {
  let language = initialLanguage;
  let theme = initialTheme;
  let settingsOpen = false;
  let openSubmenu = null;

  const menu = document.createElement('div');
  menu.className = 'settings-menu';
  menu.id = 'settings-menu';
  menu.setAttribute('role', 'menu');
  menu.hidden = true;

  const langRow = _menuRow('i-globe', 'interfaceLanguage', '界面语言');
  const themeRow = _menuRow('i-palette', 'interfaceTheme', '界面主题');

  langRow.onmouseenter = () => _showSubmenu(langRow, 'lang');
  themeRow.onmouseenter = () => _showSubmenu(themeRow, 'theme');
  langRow.onclick = () => openSubmenu === 'lang' ? _hideSubmenu() : _showSubmenu(langRow, 'lang');
  themeRow.onclick = () => openSubmenu === 'theme' ? _hideSubmenu() : _showSubmenu(themeRow, 'theme');

  const langSub = _buildSubmenu('lang');
  const themeSub = _buildSubmenu('theme');
  menu.append(langRow, themeRow);
  document.body.append(menu, langSub, themeSub);

  function _menuRow(icon, i18nKey, fallback) {
    const row = document.createElement('button');
    row.type = 'button';
    row.className = 'menu-row';
    row.setAttribute('role', 'menuitem');
    row.setAttribute('aria-haspopup', 'menu');
    row.innerHTML = `<svg class="icon"><use href="#${icon}" /></svg><span>${t(i18nKey, fallback)}</span><svg class="icon chev"><use href="#i-chevron-right" /></svg>`;
    return row;
  }

  function _buildSubmenu(kind) {
    const sub = document.createElement('div');
    sub.className = 'settings-submenu';
    sub.id = `settings-submenu-${kind}`;
    sub.setAttribute('role', 'menu');
    sub.hidden = true;
    sub.dataset.kind = kind;
    sub.addEventListener('click', (event) => {
      const item = event.target.closest('.submenu-item');
      if (!item) return;
      const value = item.dataset.value;
      if (kind === 'lang') {
        language = value;
        onLanguageChange?.(value);
      } else {
        theme = value;
        onThemeChange?.(value);
      }
      _syncSubmenus();
      toggle(false);
    });
    return sub;
  }

  function _menuOptions(kind) {
    if (kind === 'lang') {
      return [
        { value: 'zh_CN', label: t('languageZh', '中文简体') },
        { value: 'en', label: t('languageEn', 'English') },
      ];
    }
    return [
      { value: 'volt', label: t('themeVolt', '深色') },
      { value: 'archive', label: t('themeArchive', '浅色') },
    ];
  }

  function _buildMenuItems() {
    langRow.querySelector('span').textContent = t('interfaceLanguage', '界面语言');
    themeRow.querySelector('span').textContent = t('interfaceTheme', '界面主题');
    _syncSubmenus();
  }

  function _syncSubmenus() {
    for (const [sub, kind] of [[langSub, 'lang'], [themeSub, 'theme']]) {
      if (!sub) continue;
      const current = kind === 'lang' ? language : theme;
      sub.replaceChildren(..._menuOptions(kind).map((option) => {
        const item = document.createElement('button');
        item.type = 'button';
        item.className = `submenu-item${option.value === current ? ' selected' : ''}`;
        item.dataset.value = option.value;
        item.setAttribute('role', 'menuitemradio');
        item.setAttribute('aria-checked', String(option.value === current));
        item.innerHTML = `<span>${option.label}</span><svg class="icon check"><use href="#i-check" /></svg>`;
        return item;
      }));
    }
  }

  function _showSubmenu(row, kind) {
    _hideSubmenu();
    const sub = kind === 'lang' ? langSub : themeSub;
    _syncSubmenus();
    sub.hidden = false;
    row.classList.add('open');
    openSubmenu = kind;
    const rect = row.getBoundingClientRect();
    sub.style.visibility = 'hidden';
    sub.style.left = '0px';
    sub.style.top = '0px';
    const subRect = sub.getBoundingClientRect();
    let left = rect.right + 6;
    if (left + subRect.width > window.innerWidth - 8) left = rect.left - subRect.width - 6;
    sub.style.left = `${Math.max(8, left)}px`;
    sub.style.top = `${Math.max(8, Math.min(rect.top, window.innerHeight - subRect.height - 8))}px`;
    sub.style.visibility = '';
  }

  function _hideSubmenu() {
    langSub.hidden = true;
    themeSub.hidden = true;
    langRow.classList.remove('open');
    themeRow.classList.remove('open');
    openSubmenu = null;
  }

  function _positionMenu() {
    if (!anchorButton || !menu) return;
    const rect = anchorButton.getBoundingClientRect();
    menu.style.visibility = 'hidden';
    menu.style.left = '0px';
    menu.style.bottom = '0px';
    menu.style.top = 'auto';
    menu.style.left = `${Math.max(8, rect.left)}px`;
    menu.style.bottom = `${Math.max(8, window.innerHeight - rect.top + 8)}px`;
    menu.style.visibility = '';
  }

  function toggle(force, focusSubmenu) {
    const show = typeof force === 'boolean' ? force : !settingsOpen;
    settingsOpen = show;
    anchorButton?.setAttribute('aria-expanded', String(show));
    if (!show) {
      _hideSubmenu();
      menu.hidden = true;
      return;
    }
    _buildMenuItems();
    menu.hidden = false;
    _positionMenu();
    if (focusSubmenu === 'language') _showSubmenu(langRow, 'lang');
  }

  function open(focusSubmenu) {
    toggle(true, focusSubmenu);
    anchorButton?.focus();
  }

  function setPreferences({ language: nextLang, theme: nextTheme } = {}) {
    if (nextLang) language = nextLang;
    if (nextTheme) theme = nextTheme;
    if (settingsOpen) _syncSubmenus();
  }

  function onGlobalClick(event) {
    if (!settingsOpen) return;
    if (event.target.closest('#settings-menu') || event.target.closest('.settings-submenu') || (anchorButton && event.target.closest(`#${anchorButton.id}`))) return;
    toggle(false);
  }

  function onGlobalKeydown(event) {
    if (event.key === 'Escape' && settingsOpen) toggle(false);
  }

  function onWindowResize() {
    if (settingsOpen) {
      _positionMenu();
      _hideSubmenu();
    }
  }

  if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
    document.addEventListener('click', onGlobalClick);
    document.addEventListener('keydown', onGlobalKeydown);
  }
  if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
    window.addEventListener('resize', onWindowResize);
  }

  return {
    toggle,
    open,
    setPreferences,
    destroy() {
      document.removeEventListener('click', onGlobalClick);
      document.removeEventListener('keydown', onGlobalKeydown);
      window.removeEventListener('resize', onWindowResize);
      menu.remove();
      langSub.remove();
      themeSub.remove();
    },
    get isOpen() { return settingsOpen; },
  };
}
