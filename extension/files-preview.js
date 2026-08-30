(() => {
  const $ = (id) => document.getElementById(id);
  const DOC_TYPES = {
    pdf: ['PDF', 5, '#E64A3B', '#C23E31'],
    md: ['MD', 7, '#3B82F6', '#2E68C8'], markdown: ['MD', 7, '#3B82F6', '#2E68C8'], mdx: ['MDX', 5, '#3B82F6', '#2E68C8'],
    html: ['<>', 7, '#E8662A', '#C4541F'], htm: ['<>', 7, '#E8662A', '#C4541F'],
    css: ['CSS', 5, '#2D6FD6', '#2459AC'], scss: ['SCSS', 4, '#CF649A', '#A94E7C'],
    json: ['{ }', 7, '#A6824C', '#856A3E'], json5: ['{ }', 7, '#A6824C', '#856A3E'], jsonl: ['{ }', 7, '#A6824C', '#856A3E'],
    yml: ['YML', 5, '#9C5BD6', '#7E49AC'], yaml: ['YAML', 4.2, '#9C5BD6', '#7E49AC'], toml: ['TOML', 4.2, '#9C5BD6', '#7E49AC'],
    xml: ['XML', 5, '#5E8A3E', '#4A6E31'], svg: ['SVG', 5, '#E8923A', '#C4761F'],
    csv: ['CSV', 5, '#1FAE5A', '#188F4A'], tsv: ['TSV', 5, '#1FAE5A', '#188F4A'],
    sql: ['SQL', 5, '#C77D2E', '#A4661F'],
    doc: ['DOC', 5, '#2B579A', '#21457A'], docx: ['DOC', 5, '#2B579A', '#21457A'],
    xls: ['XLS', 5, '#1D6F42', '#155632'], xlsx: ['XLS', 5, '#1D6F42', '#155632'],
    ppt: ['PPT', 5, '#C43E1C', '#9E3216'], pptx: ['PPT', 5, '#C43E1C', '#9E3216'],
    log: ['LOG', 5, '#7A8290', '#626977'], txt: ['TXT', 5, '#7A8290', '#626977'],
  };
  const CODE_BADGES = {
    js: ['JS', 8, '#F0DB4F', '#1A1A1A'], mjs: ['JS', 8, '#F0DB4F', '#1A1A1A'], cjs: ['JS', 8, '#F0DB4F', '#1A1A1A'],
    jsx: ['JSX', 6, '#61DAFB', '#1A1A1A'],
    ts: ['TS', 8, '#3178C6', '#fff'], tsx: ['TSX', 6, '#3178C6', '#fff'],
    py: ['PY', 8, '#3776AB', '#FFE05B'],
    go: ['GO', 7.5, '#00ACD7', '#fff'], rs: ['RS', 8, '#CE7B43', '#fff'],
    java: ['JV', 8, '#E7700E', '#fff'], kt: ['KT', 8, '#A97BFF', '#fff'],
    rb: ['RB', 8, '#CC342D', '#fff'], php: ['PHP', 6, '#7A86B8', '#fff'],
    c: ['C', 9, '#5C6BC0', '#fff'], cpp: ['C++', 6, '#5C6BC0', '#fff'],
    vue: ['Vue', 6, '#41B883', '#fff'], swift: ['SW', 8, '#F05138', '#fff'],
    sh: ['>_', 8, '#33373D', '#3FD46A'],
  };
  const ARCHIVE_EXT = new Set(['zip', 'rar', '7z', 'gz', 'tar', 'tgz', 'dmg', 'iso']);
  const AUDIO_EXT = new Set(['mp3', 'wav', 'm4a', 'flac', 'aac']);
  const VIDEO_EXT = new Set(['mp4', 'mov', 'webm', 'mkv']);
  const IMAGE_EXT = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico']);

  let samples = [
    ['.agents', true, 'folder'], ['Natives', true, 'folder'], ['设计稿', true, 'folder'],
    ['README.md', false, 'md'], ['document.docx', false, 'docx'], ['config.json', false, 'json'],
    ['report.pdf', false, 'pdf'], ['data.csv', false, 'csv'], ['app.js', false, 'js'],
    ['main.rs', false, 'rs'], ['notes.txt', false, 'txt'], ['archive.zip', false, 'zip'],
    ['photo.png', false, 'png'], ['demo.mp4', false, 'mp4'], ['podcast.mp3', false, 'mp3'],
    ['LICENSE', false, 'other'],
  ];

  const folderSamples = {
    desktop: [
      ['Projects', true], ['Screenshots', true], ['Notes.md', false], ['todo.json', false],
    ],
    downloads: [
      ['archive.zip', false], ['setup.dmg', false], ['package.tar.gz', false], ['report.pdf', false],
    ],
    documents: [
      ['.agents', true], ['Natives', true], ['设计稿', true],
      ['README.md', false], ['document.docx', false], ['config.json', false],
      ['report.pdf', false], ['data.csv', false], ['app.js', false],
      ['main.rs', false], ['notes.txt', false], ['archive.zip', false],
      ['photo.png', false], ['demo.mp4', false], ['podcast.mp3', false],
      ['LICENSE', false],
    ],
  };

  function glyph(name, isDir) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', 'rich-glyph'); svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('aria-hidden', 'true');
    const folderColor = document.documentElement.dataset.theme === 'archive' ? '#c0714f' : '#6d8bff';
    if (isDir) {
      svg.innerHTML = `<path d="M3.6 5.5h4.4a1.2 1.2 0 0 1 .85.35l1.3 1.3a1.2 1.2 0 0 0 .85.35H20a1.6 1.6 0 0 1 1.6 1.6v8.45A1.6 1.6 0 0 1 20 19.1H4A1.6 1.6 0 0 1 2.4 17.5V6.7A1.2 1.2 0 0 1 3.6 5.5z" fill="${folderColor}"/>`;
      return svg;
    }
    const ext = (name.split('.').pop() || '').toLowerCase();
    if (DOC_TYPES[ext]) {
      const [l, fs, c, f] = DOC_TYPES[ext];
      const safeLabel = l.replace(/[&<>\"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
      svg.innerHTML = `<path d="M5 3.6A1.6 1.6 0 0 1 6.6 2H14l5 5v11.4A1.6 1.6 0 0 1 17.4 20H6.6A1.6 1.6 0 0 1 5 18.4z" fill="${c}"/><path d="M14 2l5 5h-3.4A1.6 1.6 0 0 1 14 5.4z" fill="${f}"/><text x="11.6" y="16.6" text-anchor="middle" font-family="-apple-system,sans-serif" font-weight="800" font-size="${fs}" letter-spacing="0.1" fill="#fff">${safeLabel}</text>`;
      return svg;
    }
    if (CODE_BADGES[ext]) {
      const [l, fs, c, t] = CODE_BADGES[ext];
      const safeLabel = l.replace(/[&<>\"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
      svg.innerHTML = `<rect x="3" y="3" width="18" height="18" rx="5" fill="${c}"/><text x="12" y="15.7" text-anchor="middle" font-family="-apple-system,sans-serif" font-weight="800" font-size="${fs}" fill="${t}">${safeLabel}</text>`;
      return svg;
    }
    if (ARCHIVE_EXT.has(ext)) {
      svg.innerHTML = `<rect x="4" y="3.5" width="16" height="17" rx="2.2" fill="#E0A23B"/><rect x="4" y="3.5" width="16" height="17" rx="2.2" fill="#000" opacity="0.06"/><rect x="10.6" y="3.5" width="2.8" height="17" fill="#C8862A"/><rect x="10.6" y="8" width="2.8" height="3" rx="0.5" fill="#fff8e6"/><rect x="11.4" y="11" width="1.2" height="3.4" rx="0.6" fill="#fff8e6"/>`;
      return svg;
    }
    if (AUDIO_EXT.has(ext)) {
      svg.innerHTML = `<rect x="3" y="3" width="18" height="18" rx="5" fill="#E0457B"/><g stroke="#fff" stroke-width="1.5" stroke-linecap="round"><line x1="8" y1="10" x2="8" y2="14"/><line x1="10.7" y1="8" x2="10.7" y2="16"/><line x1="13.3" y1="9.5" x2="13.3" y2="14.5"/><line x1="16" y1="7.5" x2="16" y2="16.5"/></g>`;
      return svg;
    }
    if (VIDEO_EXT.has(ext)) {
      svg.innerHTML = `<rect x="3" y="3" width="18" height="18" rx="5" fill="#7C5CE0"/><path d="M10 8.5l5 3.5-5 3.5z" fill="#fff"/>`;
      return svg;
    }
    if (IMAGE_EXT.has(ext)) {
      svg.innerHTML = `<rect x="3" y="3" width="18" height="18" rx="5" fill="#2BB6A3"/><circle cx="9" cy="9.5" r="1.6" fill="#fff"/><path d="M5 16l3.5-3.5 2.5 2.5L14.5 11 19 16z" fill="#fff"/>`;
      return svg;
    }
    const labelText = ext ? ext.slice(0, 4).toUpperCase() : (name.startsWith('.') ? name.slice(1, 5).toUpperCase() : '');
    const safeLabel = labelText.replace(/[&<>\"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
    const labelSvg = safeLabel ? `<text x="11.6" y="16.6" text-anchor="middle" font-family="-apple-system,sans-serif" font-weight="800" font-size="${safeLabel.length > 3 ? 4.2 : 5}" letter-spacing="0.1" fill="#fff">${safeLabel}</text>` : '';
    svg.innerHTML = `<path d="M5 3.6A1.6 1.6 0 0 1 6.6 2H14l5 5v11.4A1.6 1.6 0 0 1 17.4 20H6.6A1.6 1.6 0 0 1 5 18.4z" fill="#7A8290"/><path d="M14 2l5 5h-3.4A1.6 1.6 0 0 1 14 5.4z" fill="#626977"/>${labelSvg}`;
    return svg;
  }

  let currentFolder = '';
  function renderHome() {
    const box = $('entries'); box.replaceChildren(); box.classList.remove('grid');
    $('empty').hidden = true;
    $('breadcrumb').replaceChildren();
    const wrap = document.createElement('div'); wrap.className = 'home-welcome-view';
    const icon = document.createElement('div'); icon.className = 'home-welcome-icon';
    icon.innerHTML = `<svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.3 7 12 12 20.7 7"/><line x1="12" y1="22" x2="12" y2="12"/></svg>`;
    const title = document.createElement('h2'); title.className = 'home-welcome-title'; title.textContent = '请从左侧选择目录';
    const desc = document.createElement('p'); desc.className = 'home-welcome-desc muted'; desc.textContent = '从左侧侧边栏选择工作目录，或按 ⌘K 快速全局搜索文件。';
    wrap.append(icon, title, desc);
    box.append(wrap);
    $('item-count').textContent = '';
  }

  function render() {
    if (!currentFolder) {
      renderHome();
      return;
    }
    const box = $('entries'); box.replaceChildren(); box.classList.toggle('grid', $('grid-view').getAttribute('aria-pressed') === 'true');
    if (!box.classList.contains('grid')) {
      const head = document.createElement('div'); head.className = 'list-head'; head.setAttribute('aria-hidden', 'true');
      for (const label of ['', '名称', '修改时间', '大小']) head.append(Object.assign(document.createElement('span'), { textContent: label }));
      box.append(head);
    }
    samples.forEach(([name, isDir], index) => {
      const row = document.createElement('div'); row.className = 'entry'; row.tabIndex = 0; row.setAttribute('role', 'option'); row.setAttribute('aria-selected', 'false');
      const icon = Object.assign(document.createElement('span'), { className: 'entry-icon' }); icon.append(glyph(name, isDir));
      const button = Object.assign(document.createElement('button'), { className: 'entry-name', textContent: name });
      const badge = Object.assign(document.createElement('span'), { className: 'project-badge', textContent: isDir && name === 'Natives' ? 'RS' : '' }); badge.hidden = !badge.textContent;
      if (badge.textContent) button.append(badge);
      const modified = Object.assign(document.createElement('span'), { className: 'entry-meta modified', textContent: index < 3 ? '刚刚' : '2026/8/29 14:30' });
      const size = Object.assign(document.createElement('span'), { className: 'entry-meta size', textContent: isDir ? '文件夹' : `${index + 2}.4 MB` });
      row.append(icon, button, modified, size);
      row.onclick = () => { document.querySelectorAll('.entry.selected').forEach((entry) => { entry.classList.remove('selected'); entry.setAttribute('aria-selected', 'false'); }); row.classList.add('selected'); row.setAttribute('aria-selected', 'true'); $('selection-status').textContent = name; };
      row.ondblclick = () => { $('preview').hidden = false; document.querySelector('.layout').classList.add('preview-open'); $('preview-body').replaceChildren(glyph(name, isDir), Object.assign(document.createElement('h3'), { textContent: name }), Object.assign(document.createElement('p'), { className: 'muted', textContent: '静态预览不读取本地文件内容。' })); };
      box.append(row);
    });
    $('item-count').textContent = `${samples.length} 个项目`;
  }

  document.body.classList.add('static-preview');
  $('status').textContent = '从左侧选择目录或双击文件检查布局；文件操作在扩展模式中可用。';
  $('host-status').textContent = '静态预览 · 未连接本地磁盘';
  $('breadcrumb').replaceChildren();
  $('toggle-sidebar').onclick = () => {
    const collapsed = document.body.classList.toggle('sidebar-collapsed');
    $('toggle-sidebar').setAttribute('aria-pressed', String(collapsed));
  };
  let previewTheme = 'archive';
  const settingsEntry = $('settings-entry');
  let settingsMenu;
  let submenuLang;
  let submenuTheme;
  const submenuOptions = {
    lang: [{ value: 'zh_CN', label: '中文简体' }, { value: 'en', label: 'English' }],
    theme: [{ value: 'volt', label: '深色' }, { value: 'archive', label: '浅色' }],
  };
  function buildSubmenu(kind) {
    const sub = document.createElement('div');
    sub.className = 'settings-submenu';
    sub.dataset.kind = kind;
    sub.hidden = true;
    sub.addEventListener('click', (event) => {
      const item = event.target.closest('.submenu-item');
      if (!item) return;
      if (kind === 'theme') {
        previewTheme = item.dataset.value;
        document.documentElement.dataset.theme = previewTheme;
        render();
      }
      closeSettings();
    });
    return sub;
  }
  function syncSubmenus(currentLang, currentTheme) {
    for (const [sub, kind, current] of [[submenuLang, 'lang', currentLang], [submenuTheme, 'theme', currentTheme]]) {
      sub.replaceChildren(...submenuOptions[kind].map((option) => {
        const item = document.createElement('button');
        item.type = 'button';
        item.className = `submenu-item${option.value === current ? ' selected' : ''}`;
        item.dataset.value = option.value;
        item.innerHTML = `<span>${option.label}</span><svg class="icon check"><use href="#i-check" /></svg>`;
        return item;
      }));
    }
  }
  function closeSettings() {
    settingsMenu.hidden = true;
    submenuLang.hidden = true;
    submenuTheme.hidden = true;
    settingsEntry.setAttribute('aria-expanded', 'false');
  }
  settingsMenu = document.createElement('div');
  settingsMenu.className = 'settings-menu';
  settingsMenu.hidden = true;
  submenuLang = buildSubmenu('lang');
  submenuTheme = buildSubmenu('theme');
  for (const [kind, icon, label] of [['lang', 'i-globe', '界面语言'], ['theme', 'i-palette', '界面主题']]) {
    const row = document.createElement('button');
    row.type = 'button';
    row.className = 'menu-row';
    row.innerHTML = `<svg class="icon"><use href="#${icon}" /></svg><span>${label}</span><svg class="icon chev"><use href="#i-chevron-right" /></svg>`;
    const sub = kind === 'lang' ? submenuLang : submenuTheme;
    const show = () => {
      submenuLang.hidden = kind !== 'lang';
      submenuTheme.hidden = kind !== 'theme';
      syncSubmenus('zh_CN', previewTheme);
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
    };
    row.onmouseenter = show;
    row.onclick = show;
    settingsMenu.append(row);
  }
  settingsEntry.onclick = () => {
    const show = settingsMenu.hidden;
    settingsEntry.setAttribute('aria-expanded', String(show));
    if (!show) return closeSettings();
    settingsMenu.hidden = false;
    const rect = settingsEntry.getBoundingClientRect();
    settingsMenu.style.visibility = 'hidden';
    settingsMenu.style.left = '0px';
    settingsMenu.style.bottom = '0px';
    settingsMenu.style.top = 'auto';
    const menuRect = settingsMenu.getBoundingClientRect();
    settingsMenu.style.left = `${Math.max(8, rect.left)}px`;
    settingsMenu.style.bottom = `${Math.max(8, window.innerHeight - rect.top + 8)}px`;
    settingsMenu.style.visibility = '';
  };
  document.addEventListener('click', (event) => {
    if (settingsMenu.hidden) return;
    if (event.target.closest('.settings-menu') || event.target.closest('.settings-submenu') || event.target.closest('#settings-entry')) return;
    closeSettings();
  });
  document.addEventListener('keydown', (event) => { if (event.key === 'Escape' && !settingsMenu.hidden) closeSettings(); });
  document.body.append(settingsMenu, submenuLang, submenuTheme);
  $('list-view').onclick = () => { $('list-view').setAttribute('aria-pressed', 'true'); $('grid-view').setAttribute('aria-pressed', 'false'); render(); };
  $('grid-view').onclick = () => { $('list-view').setAttribute('aria-pressed', 'false'); $('grid-view').setAttribute('aria-pressed', 'true'); render(); };
  const setPreviewSize = (size) => {
    document.documentElement.dataset.gridSize = size;
    for (const val of ['small', 'medium', 'large']) {
      $(`grid-${val}`)?.setAttribute('aria-pressed', String(val === size));
    }
    $('list-view').setAttribute('aria-pressed', 'false');
    $('grid-view').setAttribute('aria-pressed', 'true');
    render();
  };
  $('grid-small').onclick = () => setPreviewSize('small');
  $('grid-medium').onclick = () => setPreviewSize('medium');
  $('grid-large').onclick = () => setPreviewSize('large');
  $('close-preview').onclick = () => { $('preview').hidden = true; document.querySelector('.layout').classList.remove('preview-open'); };
  $('quick-filter-toggle').onclick = () => {
    const open = $('quick-filter-wrap').classList.toggle('open');
    $('quick-filter-toggle').setAttribute('aria-expanded', String(open));
    if (open) $('quick-filter').focus();
  };
  const sampleUsage = {
    bytes: 1420000000,
    files: 128,
    items: [
      { name: 'Natives', isDir: true, size: 820000000 },
      { name: '设计稿', isDir: true, size: 340000000 },
      { name: 'report.pdf', isDir: false, size: 120000000 },
      { name: 'demo.mp4', isDir: false, size: 84000000 },
      { name: 'archive.zip', isDir: false, size: 36000000 },
      { name: '.agents', isDir: true, size: 18000000 },
      { name: 'app.js', isDir: false, size: 2000000 },
    ],
  };
  $('disk-usage').onclick = () => {
    const dialog = $('usage-modal');
    $('usage-title').textContent = `占用透视 · ${(sampleUsage.bytes / 1024 ** 2).toFixed(1)} MB`;
    const body = $('usage-body');
    body.replaceChildren(Object.assign(document.createElement('p'), { className: 'preview-meta muted', textContent: `/Users/demo/Documents · ${sampleUsage.files} 个文件` }));
    const list = document.createElement('div'); list.className = 'usage-list';
    sampleUsage.items.forEach((item) => {
      const row = document.createElement(item.isDir ? 'button' : 'div'); row.className = 'usage-entry';
      const label = document.createElement('span'); label.className = 'usage-entry-label'; label.append(glyph(item.name, item.isDir), document.createTextNode(` ${item.name}`));
      const meter = document.createElement('span'); meter.className = 'usage-entry-meter'; const fill = document.createElement('span'); fill.className = 'usage-entry-fill'; const ratio = item.size / sampleUsage.bytes * 100; fill.style.width = `${ratio.toFixed(2)}%`; meter.append(fill);
      const size = document.createElement('span'); size.className = 'usage-entry-size'; size.textContent = `${(item.size / 1024 ** 2).toFixed(1)} MB · ${ratio.toFixed(1)}%`; row.append(label, meter, size);
      list.append(row);
    });
    body.append(list);
    dialog.showModal();
  };
  $('usage-close').onclick = () => $('usage-modal').close();
  $('usage-back').onclick = () => {};
  document.querySelectorAll('[data-root-id]').forEach((btn) => {
    btn.onclick = () => {
      document.querySelectorAll('[data-root-id]').forEach((b) => b.classList.remove('active'));
      btn.classList.add('active');
      const rootId = btn.dataset.rootId;
      const folderName = rootId === 'desktop' ? 'Desktop' : rootId === 'downloads' ? 'Downloads' : 'Documents';
      $('breadcrumb').replaceChildren(...['⌂', 'Users', 'demo', folderName].flatMap((part, index) => {
        const crumb = Object.assign(document.createElement('button'), { className: `crumb${index === 3 ? ' last' : ''}`, textContent: part });
        return index ? [Object.assign(document.createElement('span'), { className: 'crumb-separator', textContent: ' / ' }), crumb] : [crumb];
      }));
      samples = folderSamples[rootId] || folderSamples.documents;
      render();
    };
  });
  document.querySelectorAll('.sort-tab').forEach((tab) => {
    tab.onclick = () => {
      document.querySelectorAll('.sort-tab').forEach((t) => {
        t.setAttribute('aria-pressed', 'false');
        t.classList.remove('active');
      });
      tab.setAttribute('aria-pressed', 'true');
      tab.classList.add('active');
      const sort = tab.dataset.sort;
      if (sort === 'name') {
        samples.sort((a, b) => a[0].localeCompare(b[0]));
      } else if (sort === 'mtime') {
        samples.reverse();
      } else if (sort === 'size') {
        samples.sort((a, b) => (b[1] ? 1 : 0) - (a[1] ? 1 : 0));
      }
      render();
    };
  });
  document.querySelectorAll('.action-buttons button,#new-menu,#open-trash,#follow-changes').forEach((control) => { control.disabled = true; });
  render();
})();
