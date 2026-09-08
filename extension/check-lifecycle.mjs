import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { distributableFiles } from '../scripts/extension-package.mjs';

const [manifest, background, files, nativeClient, filesHtml, filesBootstrap, filesCss, launch, dev, en, zh, spaceHtml, spaceScript, filesIcons, filesDiskUsage, filesOperations, filesPreviewControllers, filesSearch, filesShortcuts, filesEntriesRenderer, filesContextMenu, filesPreferences, filesPreviewLayout, filesToolbar, filesWorkspaceBindings, filesEditorBindings, filesEntryEffects, filesHostConnection, filesFeedback, filesWatchController, filesPaths, protocol, mainRs, workspaceStore, workspaceSchema, spacePlugins, spaceDashboard, spaceWidgetTime, spaceWidgetGreeting, spaceCss, spaceTree, sidebarController, filesSidebar] = await Promise.all([
  readFile(new URL('./manifest.json', import.meta.url), 'utf8'),
  readFile(new URL('./background.js', import.meta.url), 'utf8'),
  readFile(new URL('./files.js', import.meta.url), 'utf8'),
  readFile(new URL('./native-client.js', import.meta.url), 'utf8'),
  readFile(new URL('./files.html', import.meta.url), 'utf8'),
  readFile(new URL('./files-bootstrap.js', import.meta.url), 'utf8'),
  readFile(new URL('./files.css', import.meta.url), 'utf8'),
  readFile(new URL('./launch-workbench.mjs', import.meta.url), 'utf8'),
  readFile(new URL('./dev.mjs', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/en/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/zh_CN/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./space.html', import.meta.url), 'utf8'),
  readFile(new URL('./space.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-icons.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-disk-usage.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-operations.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-preview-controllers.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-search.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-shortcuts.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-entries-renderer.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-context-menu.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-preferences.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-preview-layout.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-toolbar.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-workspace-bindings.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-editor-bindings.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-entry-effects.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-host-connection.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-feedback.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-watch-controller.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-paths.js', import.meta.url), 'utf8'),
  readFile(new URL('../crates/native-file-host/src/protocol.rs', import.meta.url), 'utf8'),
  readFile(new URL('../crates/native-file-host/src/main.rs', import.meta.url), 'utf8'),
  readFile(new URL('../crates/native-file-host/src/workspace_store/mod.rs', import.meta.url), 'utf8'),
  readFile(new URL('../crates/native-file-host/src/workspace_store/schema.rs', import.meta.url), 'utf8'),
  readFile(new URL('./space-plugins.js', import.meta.url), 'utf8'),
  readFile(new URL('./space-dashboard.js', import.meta.url), 'utf8'),
  readFile(new URL('./plugins/widgets/time.js', import.meta.url), 'utf8'),
  readFile(new URL('./plugins/widgets/greeting.js', import.meta.url), 'utf8'),
  readFile(new URL('./space.css', import.meta.url), 'utf8'),
  readFile(new URL('./space-workspace-tree.js', import.meta.url), 'utf8'),
  readFile(new URL('./sidebar-controller.js', import.meta.url), 'utf8'),
  readFile(new URL('./files-sidebar.js', import.meta.url), 'utf8'),
]);
const enMessages = JSON.parse(en);
const zhMessages = JSON.parse(zh);
const filesBundle = [files, filesOperations, filesPreviewControllers, filesSearch, filesShortcuts, filesEntriesRenderer, filesContextMenu, filesPreferences, filesPreviewLayout, filesToolbar, filesWorkspaceBindings, filesEditorBindings, filesEntryEffects, filesHostConnection, filesFeedback, filesWatchController, filesPaths, filesSidebar].join('\n');
const scriptLocaleKeys = [...filesBundle.matchAll(/\bt\('([^']+)'/g)].map((match) => match[1]);
for (const key of new Set(scriptLocaleKeys)) {
  assert.ok(enMessages[key]?.message, `files.js locale key missing in en: ${key}`);
  assert.ok(zhMessages[key]?.message, `files.js locale key missing in zh: ${key}`);
}
assert.match(files, /(?:const fileClipboard = ops\.getClipboard\(\)|getClipboard:\s*\(\)\s*=>\s*ops\.getClipboard\(\))/, 'files.js must read clipboard state through the operations module');
const searchControllerOffset = files.indexOf('const searchController = createFilesSearch');
assert.ok(searchControllerOffset >= 0, 'files page must create its search controller');
assert.ok(searchControllerOffset < files.indexOf('const entriesRenderer = createFilesEntriesRenderer'), 'search controller must exist before the entries renderer reads it');
assert.ok(searchControllerOffset < files.indexOf('const contextMenu = createFilesContextMenu'), 'search controller must exist before the context menu reads it');
const manifestData = JSON.parse(manifest);
assert.equal(manifestData.chrome_url_overrides.newtab, 'space.html');
assert.ok(manifestData.permissions.includes('storage'), 'files page persists UI state through chrome.storage.local');
assert.doesNotMatch(manifestData.chrome_url_overrides.newtab, /newtab\.html/, 'legacy newtab page must stay retired in favor of the space page');
assert.match(filesBundle, /document\.documentElement\.lang = (?:selectedLanguage|language) === 'en'/);
assert.match(filesBundle, /chrome\.storage\?\.onChanged\?\.addListener/);
assert.doesNotMatch(files, /new Promise\([^\n]+chrome\.storage\.local\.(?:get|set)/, 'storage bootstrap must not hang behind callback-wrapped Chrome APIs');
assert.match(filesBundle, /await Promise\.race\(\[\s*chrome\.storage\.local\.get/, 'storage bootstrap must bound the MV3 Promise API');
assert.match(filesBundle, /filesSidebar\?\.setPreferences/, 'theme and language preferences must sync into the settings menu');
assert.match(filesBundle, /event\.key\.toLowerCase\(\) !== 'l'[\s\S]*openLanguageSettings/, 'files page Alt+L must open the settings menu on language');
assert.match(filesBundle, /t\('movePreviewSide'[^\n]*t\('movePreviewBelow'/, 'preview layout labels must be localized');
assert.match(filesBundle, /t\(sidebarCollapsed \? 'expandSidebar' : 'collapseSidebar'/, 'sidebar state labels must be localized');
assert.deepEqual(Object.keys(JSON.parse(en)).sort(), Object.keys(JSON.parse(zh)).sort(), 'locale keys must stay synchronized');
assert.match(nativeClient, /connectNative\(host\)/, 'real Native Client must be concentrated in the replaceable client module');
assert.doesNotMatch(files, /chrome\.runtime\.connectNative/, 'files page must not own the concrete Native Client');
assert.match(filesHtml, /<script src="files-bootstrap\.js"><\/script>/, 'files page must load its CSP-safe external bootstrap');
assert.match(filesBootstrap, /filesParams\.has\('ui-harness'\)/, 'UI Harness must enter through the formal files page');
assert.match(filesBootstrap, /filesParams\.has\('self-test'\)/, 'Self-Test must enter through the formal files page');
assert.doesNotMatch(filesBootstrap, /files-preview\.js/, 'static preview shim must stay retired');
assert.doesNotMatch(filesHtml, /<script(?![^>]*\bsrc=)[^>]*>[\s\S]*?<\/script>/i, 'MV3 extension pages must not use inline scripts');
assert.match(filesBootstrap, /import\('\.\/files\.js'\)\.catch/, 'files page must surface module bootstrap failures');
assert(files.indexOf("call('version')") < files.indexOf("call('roots')"));
assert.match(filesBundle, /const versionRequest = call\('version'\);[\s\S]{0,160}await loadUiState\(\);[\s\S]{0,160}const version = await versionRequest/, 'Host connection must not wait behind UI-state storage');
assert.match(filesBundle, /nativeHostIncompatible/);
assert.match(filesBundle, /'extract_archive',[\s\S]*'create_zip',[\s\S]*'import_begin',[\s\S]*'import_end'/, 'long-running filesystem writes must block idle disconnect');
assert.match(filesBundle, /'copy_paths',[\s\S]*'copy_image'/, 'system clipboard writes must use the write lifecycle guard');
assert.match(filesBundle, /pagehide/);
assert.match(filesBundle, /60_000/);
assert.match(filesBundle, /visibilitychange'[\s\S]{0,120}document\.hidden\) scheduleIdleDisconnect\(\)/, 'hidden pages must wait for the 60-second idle timer');
assert.match(nativeClient, /transportError/);
assert.doesNotMatch(files, /__MSG_[^*]+__/);
assert.match(filesBundle, /data-i18n/);
assert.match(filesBundle, /data-i18n-title/);
assert.match(filesBundle, /data-i18n-placeholder/);
assert.match(filesBundle, /stored === 'en' \|\| stored === 'zh_CN' \? stored : 'zh_CN'/, 'first launch must use the Chinese manifest locale');
assert.match(filesBundle, /element\.textContent = t/);
assert.match(filesHtml, /id="breadcrumb"/);
assert.doesNotMatch(filesHtml, /id="path-input"/, 'path-input must be removed in favor of breadcrumb navigation');
assert.match(filesHtml, /id="search"[^>]*placeholder="搜索文件"/);
assert.match(filesHtml, /id="new-menu"[^>]*>新建<\/button>/);
assert.match(filesHtml, /id="refresh"/, 'refresh button must be available in toolbar');
assert.doesNotMatch(filesHtml, /id="retry"/, 'duplicate retry button must stay removed');
assert.match(filesHtml, /data-i18n-aria-label="pagination" aria-label="分页"/, 'pagination must have a Chinese first-paint label');
assert.match(filesHtml, /id="modal-cancel"[^>]*>取消<\/button>/, 'modal controls must have Chinese first-paint labels');
assert.match(filesHtml, /id="settings-entry"[^>]*aria-keyshortcuts="Alt\+L"/, 'settings entry must expose the Alt+L shortcut on first paint');
assert.match(filesHtml, /href="#i-gear"/, 'settings entry must use the gear icon');
assert.match(filesHtml, /id="app-sidebar"/, 'files page must reserve a left product navigation pane');
assert.doesNotMatch(filesHtml, /id="file-sidebar"/, 'roots and preview must not be nested in a permanent right pane');
assert.doesNotMatch(filesHtml, /id="roots"|id="root-list"|id="volumes"/, 'file locations panel must stay removed');
assert.match(filesHtml, /id="preview"[^>]*hidden/, 'preview must be closed on first paint');
assert.match(filesHtml, /id="quick-filter"/, 'current-folder filtering must have a dedicated control');
assert.match(filesHtml, /id="command-search-trigger"/, 'global search must expose the command panel trigger');
assert.match(filesHtml, /data-i18n="navDocuments"/, 'left navigation must expose the documents group');
assert.match(filesHtml, /data-i18n="navSpace"[\s\S]*data-i18n="personalSpace"[\s\S]*data-i18n="navDocuments"/, 'space group must sit above the documents group in the sidebar');
assert.match(filesHtml, /id="nav-space"[^>]*href="space\.html"/, 'sidebar must link the personal space page above the documents group');
assert.match(filesHtml, /data-root-id="desktop"[\s\S]*data-root-id="downloads"[\s\S]*data-root-id="documents"/, 'documents group must contain only the three requested roots');
assert.match(spaceHtml, /app-menu-space[\s\S]*workspace-tree/, 'space page must contain the workspace tree section');
assert.match(spaceHtml, /<script type="module" src="space\.js"><\/script>/, 'space page must load its CSP-safe external script module');
assert.doesNotMatch(spaceHtml, /<script(?![^>]*\bsrc=)[^>]*>[\s\S]*?<\/script>/i, 'space page must not use inline scripts');
assert.match(nativeClient, /connectNative/, 'Native Client module must manage connection lifecycle for both files and space pages');
assert.doesNotMatch(spaceScript, /nativeMessaging/, 'space page must not declare its own nativeMessaging permission');
assert.match(spaceScript, /natives-language/);
assert.match(spaceScript, /natives-theme/);
for (const key of ['navSpace', 'personalSpace', 'spaceEmptyTitle', 'spaceEmptyDesc']) {
  assert.ok(enMessages[key]?.message, `space locale key missing in en: ${key}`);
  assert.ok(zhMessages[key]?.message, `space locale key missing in zh: ${key}`);
}
assert.match(filesCss, /body\.space-page/, 'space page must reuse the shared layout tokens');
assert.doesNotMatch(filesHtml, /id="app-favorites"|id="app-recent"|id="recent-modified"|id="changes"/, 'removed location views must not remain hidden in the DOM');
assert.doesNotMatch(filesHtml, /id="terminal"|navTerminal/, 'terminal must be removed from the file surface');
assert.doesNotMatch(files, /openTerminalSelected|call\('terminal'/, 'terminal must be removed from file actions');
assert.match(filesCss, /--sidebar-width:248px/, 'files page must default to the frozen sidebar width');
assert.match(filesCss, /\.layout\.preview-open \{ grid-template-columns:minmax\(0,1fr\) var\(--preview-width\); \}/, 'preview must open on demand at the right');
assert.match(filesCss, /\.layout\.preview-bottom\.preview-open \{[^}]*grid-template-rows:minmax\(0,1fr\) var\(--preview-height\);/, 'preview-bottom must place preview below the main file area');
assert.match(filesCss, /@media \(max-width:1180px\)[\s\S]*\.layout\.preview-bottom\.preview-open #preview \{ grid-row:2;/, 'preview-bottom must place preview below the file area');
for (const key of ['productNavigation', 'productMenu', 'personalDesktop', 'navDocuments', 'navAccount', 'navAccountType', 'filterCurrentDirectory', 'noFilterResults']) {
  assert.ok(enMessages[key]?.message);
  assert.ok(zhMessages[key]?.message);
}
assert.match(filesBundle, /openLanguageSettings: \(\) => filesSidebar\?\.openSettings\('language'\)/, 'files page must expose Alt+L language access');
assert.match(filesBundle, /function applyTheme\(/);
assert.match(filesBundle, /!\['\[', '\]'\]\.includes\(event\.key\)/, 'history navigation must support Ctrl/Meta bracket shortcuts');
assert.match(filesBundle, /natives-theme/);
assert.deepEqual(Object.keys(JSON.parse(en)).filter((key) => key.startsWith('theme')).sort(), ['theme', 'themeArchive', 'themeVolt']);
assert.deepEqual(Object.keys(JSON.parse(zh)).filter((key) => key.startsWith('theme')).sort(), ['theme', 'themeArchive', 'themeVolt']);
assert.match(filesBundle, /watchedPath/);
assert.match(filesBundle, /protocolVersion !== 1/);
assert.doesNotMatch(files, /document\.hidden \|\| nativePort \|\| reconnectTimer \|\| !session\.currentPath/, 'initial Host failures must be retried before a path is known');
assert.match(filesBundle, /(?:if \(session\.currentPath\) loadDirectory\(session\.currentPath\); else init\(\)|session\.currentPath \? loadDirectory\(session\.currentPath\) : init\(\))/, 'Host reconnect must recover the initial page bootstrap');
assert.match(filesBundle, /PAGE_SIZE = 100/);
assert.match(filesBundle, /limit: PAGE_SIZE/);
assert.doesNotMatch(files, /limit: 500|load-more|render\([^)]*, append/);
assert.match(filesBundle, /previous-page/);
assert.match(filesBundle, /next-page/);
assert.match(filesBundle, /box\.replaceChildren\(\)/);
assert.match(filesBundle, /dataTransfer\.setData\('text\/html'/, 'image drag-out must expose HTML payload');
assert.match(filesBundle, /copyImageIntoEditor/, 'local image drops must resolve through Host copy');
assert.match(filesBundle, /navigator\.clipboard\?\.writeText/, 'copy path must use text clipboard');
assert.match(filesBundle, /document\.execCommand\('copy'\)/, 'copy path must have a browser fallback');
assert.match(filesBundle, /function appendCopyPathAction/, 'preview actions must expose copy path');
assert.match(filesBundle, /appendCopyPathAction\(actions, item\)/, 'preview copy path action must be attached');
assert.match(filesBundle, /data-action="openEditor"|dataset\.action = 'openEditor'/, 'preview actions must expose editor open');
assert.match(filesBundle, /function appendEditorPreviewActions/, 'text preview must expose common actions');
assert.match(filesBundle, /appendEditorPreviewActions\(item\)/, 'text preview actions must be attached after render');
assert.match(filesBundle, /previewButton\(t\('extractArchive'/, 'archive preview must expose extraction');
assert.match(filesHtml, /id="open-trash"/, 'system trash restore entry must be visible');
assert.match(filesBundle, /call\('open_trash'\)/, 'system trash action must use the Native Host');
assert.match(filesBundle, /item\.isDir \? \[\['diskUsage', 'diskUsage'/, 'directory context menus must expose disk usage');
assert.match(filesDiskUsage, /async function show\(path = currentPath\(\)\)/, 'disk usage must accept a directory target');
assert.match(filesHtml, /id="usage-modal"/, 'disk usage must have a dedicated drill-down dialog');
assert.match(filesDiskUsage, /show\(`\$\{path\.replace/, 'disk usage directories must drill down');
assert.match(filesBundle, /createDiskUsage\(\{ \$, call, t, entryIcon, formatSize, parentAndName/, 'disk usage dialog must be composed with injected page dependencies');
assert.match(filesIcons, /export function entryIcon/, 'file glyphs must live in the decoupled icon module');
assert.match(filesIcons, /export function setIconTheme/, 'icon module must follow theme switches');
assert.match(filesBundle, /function copyFileSelected/, 'file context menus must expose system file copy');
assert.match(filesBundle, /item && !item\.isDir && item\.kind !== 'image'/, 'non-image files must expose copy file');
assert.match(filesHtml, /id="file-clipboard-status"/, 'file clipboard mode must be visible');
assert.match(filesBundle, /function clearFileClipboard/, 'file clipboard must be clearable');
assert.match(filesBundle, /clear-file-clipboard.*onclick = (?:ops\.)?clearFileClipboard/, 'file clipboard clear action must be wired');
assert.match(filesBundle, /const phase = message\.result\.event === 'archive_progress'/, 'batch progress must expose a visible phase');
assert.match(filesBundle, /setStatus\(`\$\{phase\} · \$\{completed\}\/\$\{total\}`\)/, 'batch progress must expose completed counts');
assert.doesNotMatch(files, /favorite-toggle|toggleFavorite|natives-favorites/, 'favorites from the removed file-locations panel must stay removed');
assert.match(filesBundle, /gridThumbObserver/, 'grid thumbnails must use an intersection observer');
assert.match(filesBundle, /image\/avif/, 'AVIF must remain available in the safe image preview path');
assert.match(filesBundle, /t\('editImage'/, 'AVIF preview must expose the image editor action');
assert.match(filesBundle, /t\('imageTextPrompt'/, 'image editor text input must use a localized prompt');
assert.match(filesBundle, /call\('image_preview', \{ path: item\.path \}/, 'grid thumbnails must use the bounded image preview path');
assert.match(filesCss, /\.grid-thumbnail \{ width:100%; height:70px; object-fit:contain/, 'grid thumbnails must remain bounded and use the design icon area');
assert.match(filesBundle, /function openItemFromDoubleClick/, 'double-click preview must have a dedicated fanbox flow');
assert.match(filesBundle, /row\.ondblclick = \(\) => openItemFromDoubleClick\(item\)/, 'double-click text/media must maximize preview');
assert.match(filesBundle, /event\.key === 'Enter'[\s\S]*openItemFromDoubleClick\(session\.entries\[index\]\)/, 'keyboard Enter must share the preview flow');
assert.match(filesBundle, /event\.key !== 'Enter'[\s\S]*event\.shiftKey[\s\S]*openEditorSelected/, 'Shift+Enter must open the selected item in an editor');
assert.match(filesBundle, /setPendingSelectionPath\(`\$\{session\.currentPath\.replace/, 'new entries must be selected after creation');
assert.match(filesBundle, /setPendingSelectionPath\((?:moved|copied)\.at\(-1\)/, 'dropped moves and copies must select the last written item');
assert.match(filesBundle, /setPendingSelectionPath\(result\?\.path\)/, 'created archives must be selected after writing');
assert.match(filesBundle, /const previewPending = Boolean\(pendingSelectionPath && visiblePaths\.has\(pendingSelectionPath\)\)/, 'located search results must trigger a preview selection');
assert.match(filesBundle, /if \(previewPending\) renderSelection\(\)/, 'located search results must render the selected preview');
assert.match(filesBundle, /const historyScroll = new Map/, 'navigation must keep bounded scroll history');
assert.match(filesBundle, /pendingScrollTop = historyScroll\.get\(path\) \?\? 0/, 'back-forward navigation must restore scroll position');
assert.match(filesBundle, /event\.key === 'Escape'[\s\S]*cancelActiveSearches\(\)/, 'Escape must clear the active search query');
assert.match(filesBundle, /const failedPaths = errors\.map/, 'batch retries must retain only failed source paths');
assert.match(filesBundle, /const viewport = document\.querySelector\('\.content'\); if \(viewport\) viewport\.scrollTop = scrollTop/, 'batch retries must restore scroll position');
assert.match(filesBundle, /const candidates = failedPaths\.length \? failedPaths : cancelled \? items\.map/, 'duplicate cancellation must retain actionable selection');
assert.match(filesBundle, /const failedPaths = \[\.\.\.errors, \.\.\.skipped\]/, 'copy/move cancellation must retain failed and skipped paths');
assert.match(filesBundle, /if \(!dialog\.open\) dialog\.showModal\(\)/, 'disk usage drill-down must reuse the open dialog');
assert.match(filesBundle, /migrateTrackedPath/, 'rename must migrate shortcut paths');
assert.match(filesBundle, /method === 'move' && completed\.length === items\.length|migrateBatchPaths\(items\.map/, 'moves must migrate shortcut paths');
assert.match(filesBundle, /migrateBatchPaths\(paths, moved, errors\)/, 'drag moves must migrate shortcut paths');
assert.match(filesBundle, /method === 'move_batch'[\s\S]*(?:migrateBatchPaths|onMoveBatchResult)/, 'all partial move callers must migrate successful paths');
assert.match(filesBundle, /method === 'trash_batch'[\s\S]*(?:rememberFailedOperation|onTrashBatchErrors)/, 'trash failures must retain retryable paths');
assert.match(filesBundle, /const failed = lastFailedOperation\?\.method === 'trash_batch'/, 'trash retries must refresh the current directory');
assert.match(filesBundle, /scrollTop = document\.querySelector\('\.content'\)\?\.scrollTop/, 'trash retries must preserve scroll position');
assert.match(filesBundle, /session\.selectedPaths = new Set\(\[\.\.\.failed\]/, 'trash retries must reselect remaining failures');
assert.match(filesBundle, /clip\.mode === 'move' && completed\.length === clip\.paths\.length|migrateBatchPaths\(clip\.paths/, 'paste moves must migrate shortcut paths');
assert.match(filesBundle, /session\.currentPath !== watchedDirectory/, 'stale watcher reloads must be ignored after navigation');
assert.match(filesHtml, /id="scope-toggle"/, 'search scope must be visible');
assert.match(filesHtml, /id="grid-view"[^>]*aria-keyshortcuts="Control\+Shift\+G Meta\+Shift\+G"/, 'view buttons must expose the grid/list shortcut');
assert.match(filesHtml, /id="settings-entry"/, 'settings entry must be visible in the sidebar');
assert.match(filesBundle, /natives-language/, 'language preference must persist');
assert.match(filesBundle, /fetch\(`_locales\/\$\{(?:selectedLanguage|language)\}\/messages\.json`\)/, 'language switch must load bundled locale data');
assert.match(filesCss, /@media \(max-width:560px\)/, 'file layout must adapt to narrow viewports');
assert.match(filesCss, /\.preview-image[^\n]*background-image:linear-gradient/, 'image previews must show a transparency checkerboard');
assert.match(filesCss, /@media \(max-width:1180px\)[\s\S]*\.layout\.preview-bottom\.preview-open #preview \{ grid-row:2/, 'preview-bottom must place preview below the file list');
assert.match(filesCss, /@media \(max-width:560px\)[\s\S]*\.entry-meta,\.entry-source,\.project-badge \{ display:none/, 'very narrow lists must collapse secondary metadata');
assert.match(filesBundle, /function toggleSearchScope/, 'search scope must be keyboard and button controllable');
assert.match(filesBundle, /event\.shiftKey && shortcut === 'g'.*viewMode = viewMode === 'grid' \? 'list' : 'grid'/, 'view mode must toggle from the keyboard');
assert.match(filesBundle, /currentDirectorySearch/, 'search scope labels must be bilingual and semantically distinct');
assert.match(filesBundle, /event\.key === 'Tab' && !event\.shiftKey/, 'search scope must support forward Tab shortcut');
assert.match(filesBundle, /storageSet\('natives-sort-by', session\.sortBy\)/, 'sort selection must persist');
assert.match(filesBundle, /storageSet\('natives-view-mode', 'list'\)/, 'list view must persist');
assert.match(filesBundle, /storageSet\('natives-view-mode', 'grid'\)/, 'grid view must persist');
assert.match(filesBundle, /storageSet\('natives-show-hidden'/, 'hidden-file preference must persist');
assert.match(filesBundle, /natives-recursive-search/, 'recursive search preference must persist');
assert.match(filesBundle, /editorViewStates\.set\(editorState\.path/, 'editor view state must be remembered by path');
assert.match(filesBundle, /editorViewStates\.get\(item\.path\)/, 'editor view state must restore by path');
assert.match(filesBundle, /migrateEditorViewPath/, 'editor view state must follow path changes');
assert.match(filesBundle, /function rememberEditorViewState/, 'closing preview must remember editor view');
assert.match(filesBundle, /function resetPreviewNow\(\) \{ rememberEditorViewState\(\);/, 'preview reset must capture editor view first');
assert.match(filesHtml, /id="toggle-preview-layout"[^>]*aria-pressed/, 'preview layout control must expose pressed state');
assert.match(filesBundle, /function syncPreviewLayoutControls/, 'preview layout controls must sync after restore');
assert.match(filesBundle, /setSearchQuery\(''\)[\s\S]{0,120}setGlobalMode\(false\)[\s\S]{0,120}loadDirectory\(session\.currentPath\)/, 'clearing search must sync scope control');
assert.match(filesBundle, /excerpt\.setAttribute\('role', 'button'\)/, 'search match summaries must be keyboard accessible');
assert.match(filesBundle, /excerpt\.click\(\)/, 'search match summaries must activate from keyboard');
assert.match(filesBundle, /event\.key === 'Enter'[\s\S]{0,120}dispatchEvent\(new MouseEvent\('dblclick'/, 'Enter on a match must open preview context');
assert.match(filesBundle, /entry-source/, 'global search results must expose source paths');
assert.match(filesBundle, /searchTruncated/, 'search truncation must be tracked and surfaced');
assert.match(filesBundle, /if \(searchQuery\) search\(searchQuery\); else \{ searchToken\+\+; cancelActiveSearches\(\); loadDirectory\(session\.currentPath\); \}/, 'clearing search must cancel stale requests before loading directory');
assert.match(filesBundle, /call\('extract_archive'/, 'archive extraction must use the Native Host');
assert.match(filesBundle, /if \(!item && session\.currentPath\)/, 'blank-area context menu must expose directory actions');
assert.match(filesBundle, /dataset\.action = 'importFiles'/, 'blank-area context menu must expose file import');
assert.match(filesBundle, /previous\.dataset\.action = 'pdfPrevious'/, 'PDF preview must expose previous-page control');
assert.match(filesBundle, /nextButton\.dataset\.action = 'pdfNext'/, 'PDF preview must expose next-page control');
assert.match(filesBundle, /frame\.src = `\$\{previewObjectUrl\}#page=\$\{page\}/, 'PDF paging must update the controlled viewer anchor');
assert.match(filesBundle, /previous\.disabled = page <= 1/, 'PDF previous-page control must disable at the first page');
assert.match(filesBundle, /nextButton\.disabled = totalPages > 0 && page >= totalPages/, 'PDF next-page control must disable at the last known page');
assert.match(filesBundle, /extractArchive/, 'archive context action must be exposed');
assert.match(filesBundle, /function createZip/, 'archive creation function must be wired');
assert.match(filesBundle, /call\('create_zip'/, 'archive creation must use the Native Host');
assert.match(filesBundle, /call\('duplicate_batch'/, 'duplicate actions must use the cancellable batch protocol');
assert.match(filesBundle, /archive_progress/, 'archive progress events must be consumed by the page');
assert.match(filesBundle, /contentSearchUnavailable/, 'content search capability gaps must be surfaced');
assert.match(filesHtml, /id="retry-operation"/, 'batch failures must expose a retry action');
assert.match(filesBundle, /function retryFailedOperation/, 'batch failure retry must be wired');
assert.match(filesBundle, /failedFiles\.push\(file\)/, 'import failures must retain retryable files');
assert.match(filesBundle, /const importTarget = `\$\{t\('importDestination'/, 'imports must disclose their destination');
assert.match(filesBundle, /\$\{importTarget\} · \$\{importedCount\}/, 'import completion must include destination and counts');
assert.match(filesBundle, /rememberFailedOperation\(\{ method: 'move_batch'/, 'drag moves must retain failed paths');
assert.match(filesBundle, /rememberFailedOperation\(\{ method: 'copy_batch'/, 'URI imports must retain failed paths');
assert.match(filesBundle, /else if \(urls\) await copyDroppedUris\(urls, item\.path\)/, 'directory drops must accept ordinary file URIs');
assert.match(filesBundle, /if \(event\.dataTransfer\.files\.length\) await ops\.importFileList[\s\S]{0,120}else if \(raw\.startsWith\('\['\)\)/, 'external file drops must bypass internal JSON path handling');
assert.match(filesBundle, /t\('dropHint', '释放以导入或移动文件'\)/, 'drag targets must expose a visible drop hint');
assert.match(filesBundle, /input\.multiple = true;[\s\S]{0,80}input\.accept = 'image\/\*'/, 'markdown image picker must support multi-selection');
assert.match(filesBundle, /if \(!await (?:ops\.)?importImageIntoEditor\(file, editor\)\) break/, 'cancelled image imports must stop the remaining batch');
assert.match(filesBundle, /const files = \[\.\.\.\(event\.dataTransfer\?\.files \|\| \[\]\)\][\s\S]*slice\(0, 20\)/, 'editor drops must accept a bounded image batch');
assert.match(filesBundle, /setData\('text\/html'[\s\S]*htmlEscape/, 'file drags must expose escaped HTML links');
assert.match(filesBundle, /method: 'image-import'[\s\S]*editorPath/, 'failed editor image imports must retain retry context');
assert.match(filesBundle, /method: 'image-copy'[\s\S]*editorPath/, 'failed editor image copies must retain retry context');
assert.match(filesBundle, /begun\?\.renamed/, 'imports must surface Host conflict renames');
assert.match(filesBundle, /countRenamedPaths\(paths, completed\)/, 'batch retries must surface renamed outputs');
assert.match(filesBundle, /countRenamedPaths\(clip\.paths, completed\)/, 'clipboard pastes must surface renamed outputs');
assert.match(filesBundle, /migrateBatchPaths\(paths, moved, errors\)/, 'partial drag moves must migrate successful paths');
assert.match(filesBundle, /const finalName = result\?\.path \? parentAndName\(result\.path\)\.name/, 'single renames must expose the final name');
assert.match(filesBundle, /restore:\s*\{ selection: \[\.\.\.importSelection\], scrollTop: importScrollTop \}/, 'import retries must retain selection and scroll position');
assert.match(filesBundle, /dataset\.action = 'createArchive'/, 'archive creation must be available from context menus');
assert.match(filesBundle, /event\.key !== 'Enter'[\s\S]*session\.selectedPaths\.size !== 1[\s\S]*openEditorSelected/, 'Cmd/Ctrl+Enter must open the selected item in an editor');
assert.match(filesBundle, /activeElement\?\.classList\.contains\('pdf-preview'\)/, 'PDF viewer must retain native find shortcut');
assert.match(filesBundle, /findButton\.dataset\.action = 'pdf-find'/, 'PDF viewer must expose a visible find action');
assert.match(filesBundle, /const pageStep = Math\.max\(1, Math\.floor\(\(entries\.clientHeight/, 'entry paging must follow viewport size');
assert.doesNotMatch(files, /recentModified|recent-modified|recent_files/, 'recent-modified feature must stay removed');
assert.match(filesBundle, /const watchedDirectory = session\.currentPath/, 'watch refresh must capture the watched directory');
assert.match(filesBundle, /markChangedPath\(changedPath, message\.result\.kind/, 'watch events must preserve their change kind');
assert.match(filesBundle, /if \(session\.currentPath !== watchedDirectory\) return;[\s\S]*const scrollTop/, 'stale watcher events must not refresh a new directory');
assert.match(filesBundle, /const refresh = searchQuery \? (?:searchController\.)?search\(searchQuery\) : loadDirectory\(watchedDirectory\)/, 'watch refresh must preserve active search context');
assert.match(filesBundle, /viewport\.scrollTop = scrollTop/, 'watch refresh must preserve scroll position');
assert.match(filesBundle, /previewPath && !visiblePaths\.has\(previewPath\)[\s\S]{0,120}resetPreviewNow\(\)/, 'watch refresh must close clean previews for deleted files');
assert.match(filesBundle, /message\.result\.kind === 'removed'[\s\S]{0,220}(?:resetPreviewNow|resetPreview)\(\)/, 'removed watcher events must close stale previews in search mode');
assert.doesNotMatch(files, /changeLog|renderChanges|openChangePath/, 'removed file-location change inbox must stay removed');
assert.match(filesBundle, /storageSet\('natives-last-path', path\)/, 'navigation must persist the last directory');
assert.match(filesBundle, /storageGet\('natives-last-path', ''\)/, 'startup must restore the last directory preference');
assert.match(filesBundle, /result\?\.found && result\.isDir\) navigate\(stored, false\)/, 'restored paths must be validated as authorized directories');
assert.match(filesBundle, /activePreviewId && isHostConnected\(\)/, 'preview cancellation must not reconnect a disconnected Host');
assert.match(filesBundle, /__nativesPreviewCleanup/, 'Host disconnect must clear clean preview state');
assert.match(filesBundle, /selectedPaths\.has\(changedPath\)[\s\S]{0,120}renderPreviewSelection\(\)/, 'binary previews must refresh after external file changes');
for (const key of ['rotateLeft', 'rotateRight', 'flipHorizontal', 'flipVertical', 'imagePen', 'imageCrop', 'applyCrop']) {
  assert.ok(JSON.parse(en)[key]?.message, `${key} must be localized in en`);
  assert.ok(JSON.parse(zh)[key]?.message, `${key} must be localized in zh`);
}
assert.doesNotMatch(files, /refreshTreeNode|expandedTreePaths|treeRefreshers|function treeNode/, 'directory-tree production code must stay removed');
assert.match(filesCss, /:root\[data-theme="volt"\][\s\S]*color-scheme:dark/, 'files surface must provide the fixed dark Volt theme');
assert.match(filesCss, /:root\[data-theme="archive"\][\s\S]*color-scheme:light/, 'files surface must provide the warm Archive theme');
assert.match(filesHtml, /<html lang="zh-CN" data-theme="archive">/, 'the first paint must match the warm files design baseline');
assert.match(filesBundle, /let selectedTheme = 'archive'/, 'the files design baseline must default to Archive');
assert.match(filesBundle, /className = 'list-head'/, 'list view must expose the design column header without creating a false file option');
assert.match(filesCss, /\.entries\.grid \.entry-icon \.rich-glyph \{ width:64px; height:64px;/, 'medium grid file glyphs must match the design scale');
assert.match(filesCss, /\.entries\.grid \.entry \{[\s\S]*?border:1px solid transparent;[\s\S]*?background:transparent;/, 'grid items must use the flat design surface');
assert.match(filesCss, /:root\[data-theme="archive"\] body \{[\s\S]*?background-size:4px 4px;/, 'Archive must keep the frozen paper texture');
assert.doesNotMatch(filesCss, /:root\[data-theme="index"\]/, 'Index theme must stay removed');
assert.match(filesCss, /@media \(max-width:1180px\)/, 'files toolbar must reflow at medium widths');
assert.match(filesHtml, /class="nav-buttons"/, 'files toolbar must group navigation controls');
assert.match(filesHtml, /id="empty"[^>]*role="status"[^>]*aria-live="polite"/, 'empty state must be announced accessibly');
assert.match(filesCss, /scrollbar-gutter:\s*stable/, 'file panes must reserve scrollbar space');
assert.match(filesCss, /\.entries\.drop-target\s*\{[^}]*outline:/, 'file area drop target must be visible');
assert.match(filesBundle, /closest\('#new-popover'\).*hidden = true/, 'new menu must close when clicking outside');
assert.match(filesCss, /\.nav-buttons,\.toolbar-controls,\.view-buttons/, 'view controls must stay on one horizontal row');
assert.match(filesCss, /\[hidden\] \{ display:none !important; \}/, 'hidden menus must not cover the workspace');
assert.match(filesBundle, /popover\.style\.left = `\$\{Math\.max\(8, Math\.min\(anchor\.right - width/, 'new menu must anchor to its trigger');
assert.match(filesCss, /@media \(max-width:1180px\)[\s\S]*\.layout\.preview-open.*grid-template-columns:minmax\(0,1fr\)/, 'files content must collapse before fixed panels overflow');
assert.match(filesCss, /@media \(max-width:760px\)/, 'files layout must remain usable on narrow widths');
assert.match(filesCss, /@media \(max-width:760px\)[\s\S]*\.entry \{ grid-template-columns:28px minmax\(0,1fr\)/, 'file rows must compress before the content column overflows');
assert.match(filesCss, /\.entry:focus-visible/, 'file keyboard focus must be theme-visible');
assert.match(filesCss, /\.entries\.grid \.entry \{[^}]*background:transparent/, 'grid entries must preserve the flat design surface');
assert.match(filesBundle, /event\.key !== 'Escape'[\s\S]*?is-maximized[\s\S]*?maximize-preview/, 'maximized preview must support Escape restore');
assert.match(filesBundle, /t\('createdAt', '创建'\)/, 'preview metadata must expose creation time when available');
assert.match(filesBundle, /className = 'preview-meta-field'/, 'preview metadata must render separate accessible fields');
assert.match(filesCss, /\.preview-actions,\.editor-toolbar,\.markdown-tools \{ display:flex; flex-wrap:wrap/, 'preview actions must wrap in narrow panes');
assert.match(filesBundle, /function stopFollowOnManual\(\)/, 'manual navigation must stop follow mode');
assert.match(filesBundle, /if \(session\.currentPath && followChanges && !pendingSelectionPath\) watchController\.stopFollowOnManual\(\)/, 'manual directory navigation must stop follow mode');
assert.match(filesBundle, /for \(const \[path, value\] of changedPaths\)/, 'renames and moves must migrate transient change heat paths');
for (const key of ['pagination', 'previousPage', 'nextPage']) {
  assert.ok(JSON.parse(en)[key]?.message);
  assert.ok(JSON.parse(zh)[key]?.message);
}
for (const key of ['language', 'currentDirectorySearch', 'previewUnavailable', 'clipboardUnavailable', 'clipboardCleared', 'invalidPath']) {
  assert.ok(JSON.parse(en)[key]?.message);
  assert.ok(JSON.parse(zh)[key]?.message);
}
assert.doesNotMatch(background, /connectNative|setTimeout|pending|nativePort/);
assert.match(launch, /files\.html/);
assert.match(dev, /fs\/promises/);
assert.match(dev, /buildExtension\(devExtension\)/, 'dev must use the same distribution bytes as the release budget');
assert.match(dev, /generateKeyPairSync/);
assert.match(dev, /install-native-host\.mjs/);
assert.match(dev, /Chrome will start the Host on demand/);
assert.match(dev, /setInterval\(\(\) => \{\}, 2 \*\* 31 - 1\)/, 'dev must stay alive until explicitly stopped');
assert.match(dev, /\['SIGINT', 'SIGTERM'\]/, 'dev must stop on Ctrl+C or termination');
assert.doesNotMatch(dev, /cleanupManifest|process\.on\('exit'|NATIVES_OPEN_BROWSER|--load-extension|chrome:\/\/extensions/, 'dev must persist Host registration and never launch Chrome');
assert.ok(!distributableFiles().includes('native-client.test.mjs'), 'test code must not enter the extension');
// --- ADR-0024: workspace authority + shell ---
assert.match(protocol, /"workspace_session"[\s\S]*"workspace_snapshot"[\s\S]*"workspace_create"[\s\S]*"workspace_delete"[\s\S]*"workspace_widget_upsert"[\s\S]*"workspace_save_from_tabliss"[\s\S]*"settings_get"[\s\S]*"settings_set"/, 'workspace method family must be whitelisted');
assert.match(workspaceSchema, /CREATE TABLE IF NOT EXISTS settings/, 'settings table must exist');
assert.match(workspaceSchema, /CREATE TABLE IF NOT EXISTS workspaces/, 'workspaces table must exist');
assert.match(workspaceSchema, /CREATE TABLE IF NOT EXISTS workspace_open_tabs/, 'workspace_open_tabs table must exist');
assert.match(workspaceSchema, /CREATE TABLE IF NOT EXISTS workspace_widgets/, 'workspace_widgets table must exist');
assert.match(workspaceSchema, /CREATE TABLE IF NOT EXISTS workspace_templates/, 'workspace_templates table must exist');
assert.match(workspaceSchema, /deleted_at TEXT/, 'soft delete column must exist');
assert.match(workspaceStore, /RevisionConflict/, 'revision check must be exposed');
assert.match(workspaceSchema, /natives\.db/, 'db path must be ~/.natives/natives.db');
assert.match(mainRs, /fn workspace_dispatch/, 'workspace dispatch must be wired');
assert.match(mainRs, /workspace_store::default_db_path/, 'workspace store must open from the default path');
assert.match(spaceHtml, /dashboard-host/, 'space shell must have a dashboard host');
assert.match(spaceHtml, /workspace-tree/, 'space shell must have a workspace tree');
assert.match(spaceHtml, /app-menu-section-title[\s\S]*id="workspace-create"/, 'workspace creation must be the title-row icon');
assert.match(spaceHtml, /id="workspace-menu"[\s\S]*data-action="rename"[\s\S]*data-action="delete"/, 'workspace actions must use a visible context menu');
assert.match(spaceTree, /oncontextmenu/, 'workspace names must support right-click actions');
assert.match(spaceTree, /i-more/, 'workspace rows must expose a hover More action');
assert.doesNotMatch(spaceTree, /\b(?:prompt|confirm)\s*\(/, 'workspace actions must not use browser prompt commands');
assert.match(filesHtml, /class="brand"[\s\S]*id="toggle-sidebar"/, 'files sidebar toggle must live in the shared sidebar');
assert.match(spaceHtml, /id="space-toggle-sidebar-btn"/, 'personal space must expose the dashboard sidebar toggle');
assert.match(spaceHtml, /id="space-toggle-widgets-btn"/, 'personal space must expose the dashboard widget visibility toggle');
assert.match(filesCss, /body\.sidebar-collapsed #app-sidebar\s*\{[^}]*background:var\(--surface-2\);/, 'files collapsed sidebar must have solid surface-2 background');
assert.match(filesSidebar, /createSidebarController/, 'file manager must reuse the shared sidebar controller');
assert.match(spaceScript, /createSidebarController/, 'personal space must reuse the shared sidebar resize controller');
assert.match(sidebarController, /sidebar-collapsed/, 'shared sidebar controller must own collapse state');
assert.match(spaceHtml, /inspector/, 'space shell must have an inspector');
assert.match(spaceCss, /dashboard-host/, 'space CSS must style the dashboard');
assert.match(spaceCss, /inspector-resizer/, 'inspector must be resizable');
assert.match(spaceCss, /@media \(max-width:1180px\)/, 'narrow screens must go full-cover inspector');
assert.match(spacePlugins, /WIDGET_KEYS/, '29 widget key registry must exist');
assert.match(spacePlugins, /BACKGROUND_KEYS/, '9 background key registry must exist');
assert.match(spacePlugins, /sanitizeHtml/, 'HTML DOM allowlist sanitizer must exist');
assert.match(spaceDashboard, /attachShadow/, 'dashboard must render inside Shadow DOM');
assert.match(spaceScript, /BroadcastChannel\('natives-workspace'\)/, 'revision invalidation must use BroadcastChannel');
assert.match(spaceWidgetTime, /widget\/time/, 'core time widget must be registered');
assert.match(spaceWidgetGreeting, /widget\/greeting/, 'core greeting widget must be registered');
assert.doesNotMatch(spacePlugins, /widget\/nba/, 'broken NBA widget must be excluded');
console.log('extension lifecycle checks passed');
