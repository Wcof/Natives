(function () {
  const params = new URLSearchParams(location.search);
  const mode = params.has('self-test') ? 'self-test' : 'ui-harness';
  const steps = [];
  const responses = [];
  const root = '/tmp/natives-ui-harness';
  const data = new Map([
    [`${root}/alpha.txt`, { content: 'alpha\n', mtime: Date.now(), kind: 'text' }],
    [`${root}/child/beta.txt`, { content: 'beta\n', mtime: Date.now(), kind: 'text' }],
    [`${root}/desktop/desktop.txt`, { content: 'desktop\n', mtime: Date.now(), kind: 'text' }],
    [`${root}/downloads/download.txt`, { content: 'download\n', mtime: Date.now(), kind: 'text' }],
    [`${root}/documents/document.txt`, { content: 'document\n', mtime: Date.now(), kind: 'text' }],
  ]);
  const dirs = new Set([root, `${root}/child`, `${root}/desktop`, `${root}/downloads`, `${root}/documents`]);

  function children(path) {
    const prefix = `${path.replace(/\/$/, '')}/`;
    const names = new Map();
    for (const directory of dirs) if (directory.startsWith(prefix) && !directory.slice(prefix.length).includes('/')) names.set(directory.slice(prefix.length), true);
    for (const file of data.keys()) if (file.startsWith(prefix) && !file.slice(prefix.length).includes('/')) names.set(file.slice(prefix.length), false);
    return [...names].map(([name, isDir]) => ({ name, path: `${prefix}${name}`, isDir, size: isDir ? 0 : data.get(`${prefix}${name}`).content.length, mtime: isDir ? Date.now() : data.get(`${prefix}${name}`).mtime, kind: isDir ? 'directory' : data.get(`${prefix}${name}`).kind }));
  }
  function mockPort() {
    const messages = [];
    const disconnects = [];
    let closed = false;
    const port = {
      onMessage: { addListener(listener) { messages.push(listener); } },
      onDisconnect: { addListener(listener) { disconnects.push(listener); } },
      postMessage(request) {
        queueMicrotask(() => {
          if (closed) return;
          (globalThis.__NATIVES_TEST_HOST_REQUESTS__ ||= []).push({ method: request.method, id: request.id });
          let result;
          const p = request.params || {};
          if (request.method === 'version') result = { protocolVersion: 1 };
          else if (request.method === 'roots') result = [
            { id: 'home', name: 'Harness', path: root },
            { id: 'desktop', name: 'Desktop', path: `${root}/desktop` },
            { id: 'downloads', name: 'Downloads', path: `${root}/downloads` },
            { id: 'documents', name: 'Documents', path: `${root}/documents` },
          ];
          else if (request.method === 'list_dir') result = { entries: children(p.path).sort((a, b) => a.name.localeCompare(b.name)), hasMore: false };
          else if (request.method === 'search') result = { entries: [...data].filter(([path, item]) => path.startsWith(`${p.path}/`) && path.toLowerCase().includes(String(p.query).toLowerCase())).map(([path, item]) => ({ name: path.split('/').pop(), path, isDir: false, size: item.content.length, mtime: item.mtime, kind: item.kind })), hasMore: false };
          else if (request.method === 'stat') result = { found: dirs.has(p.path), isDir: dirs.has(p.path) };
          else if (request.method === 'read_file') result = { content: data.get(p.path)?.content || '', kind: 'text', mtime: data.get(p.path)?.mtime || Date.now() };
          else if (request.method === 'watch_start') result = { watchId: 'harness-watch' };
          else if (request.method === 'create_folder') { dirs.add(`${p.parent.replace(/\/$/, '')}/${p.name}`); result = { path: `${p.parent.replace(/\/$/, '')}/${p.name}` }; }
          else if (request.method === 'write_file') { data.set(`${p.parent.replace(/\/$/, '')}/${p.name}`, { content: atob(p.data || ''), mtime: Date.now(), kind: 'text' }); result = { path: `${p.parent.replace(/\/$/, '')}/${p.name}` }; }
          else if (request.method === 'rename') { const oldPath = p.path; const nextPath = `${oldPath.slice(0, oldPath.lastIndexOf('/'))}/${p.name}`; const item = data.get(oldPath); if (item) { data.delete(oldPath); data.set(nextPath, item); } if (dirs.has(oldPath)) { dirs.delete(oldPath); dirs.add(nextPath); } result = { path: nextPath }; }
          else if (request.method === 'trash_batch') { for (const path of p.paths || []) { data.delete(path); dirs.delete(path); for (const child of [...data.keys()]) if (child.startsWith(`${path}/`)) data.delete(child); for (const child of [...dirs]) if (child.startsWith(`${path}/`)) dirs.delete(child); } result = { count: (p.paths || []).length, errors: [] }; }
          else if (request.method === 'watch_stop' || request.method.endsWith('_cancel')) result = { cancelled: true };
          else result = {};
          messages.forEach((listener) => listener({ id: request.id, ok: true, result }));
        });
      },
      disconnect() { if (closed) return; closed = true; disconnects.forEach((listener) => listener()); },
    };
    return port;
  }

  if (mode === 'ui-harness') globalThis.__NATIVES_DEV_NATIVE_CONNECT__ = () => mockPort();
  globalThis.__NATIVES_TEST_RESPONSE__ = (message) => responses.push({ id: message.id, ok: message.ok === true, error: message.ok ? undefined : message.error, resultKeys: message.result && typeof message.result === 'object' ? Object.keys(message.result) : [] });

  function pageState() {
    return {
      url: location.href,
      language: document.documentElement.lang,
      path: document.querySelector('#breadcrumb .crumb.last')?.textContent || '',
      status: document.querySelector('#status')?.textContent || '',
      host: document.querySelector('#host-status')?.textContent || '',
      entries: [...document.querySelectorAll('#entries .entry')].map((row) => ({ path: row.dataset.path, selected: row.getAttribute('aria-selected') === 'true' })),
      view: document.querySelector('#entries')?.className || '',
    };
  }
  function record(name, pass, detail = '') { steps.push({ name, pass, detail, pageState: pageState() }); }
  function click(selector) { const element = document.querySelector(selector); if (!element) throw new Error(`missing ${selector}`); element.dispatchEvent(new MouseEvent('click', { bubbles: true })); return element; }
  function input(selector, value) { const element = document.querySelector(selector); if (!element) throw new Error(`missing ${selector}`); element.value = value; element.dispatchEvent(new Event('input', { bubbles: true })); return element; }
  async function waitFor(predicate, timeout = 4000) {
    const start = Date.now();
    while (Date.now() - start < timeout) { if (predicate()) return; await new Promise((resolve) => setTimeout(resolve, 25)); }
    throw new Error('timeout');
  }
  function showReport(report) {
    const pre = document.createElement('pre'); pre.id = 'natives-test-report'; pre.textContent = JSON.stringify(report, null, 2); pre.style.cssText = 'position:fixed;inset:auto 8px 8px 8px;max-height:35vh;overflow:auto;background:#111;color:#9f9;padding:8px;z-index:99999'; document.body.append(pre);
    globalThis.__NATIVES_TEST_REPORT__ = report;
    document.title = `${report.status} · Natives ${mode}`;
  }
  async function run() {
    const report = { mode, status: 'FAIL', steps, input: { root: mode === 'ui-harness' ? root : params.get('self-test-root') || '' }, pageState: pageState(), host: [], disk: mode === 'ui-harness' ? 'mock filesystem' : 'caller must verify the supplied temporary directory' };
    try {
      await waitFor(() => document.querySelectorAll('#entries .entry').length > 0 && document.querySelector('#host-status')?.textContent);
      record('connection/loading', true);
      click('[data-root-id="downloads"]');
      await waitFor(() => document.querySelector('#breadcrumb .crumb.last')?.textContent === 'downloads');
      await waitFor(() => [...document.querySelectorAll('#entries .entry')].some((row) => row.dataset.path.endsWith('/download.txt')));
      record('app-menu/navigation', true);
      click('#up');
      await waitFor(() => document.querySelector('#breadcrumb .crumb.last')?.textContent !== 'downloads');
      if (mode === 'self-test' && !params.get('self-test-root')) throw new Error('self-test-root is required for a real temporary-directory test');
      if (mode === 'self-test' && params.get('self-test-root')) {
        await waitFor(() => document.querySelectorAll('#entries .entry').length > 0);
        record('real-host/temp-root', true);
      }
      const first = document.querySelector('#entries .entry');
      first.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      record('selection/click', first.getAttribute('aria-selected') === 'true');
      input('#quick-filter', 'alpha');
      await waitFor(() => document.querySelectorAll('#entries .entry:not([hidden])').length === 1);
      record('current-folder/filter', document.querySelector('#entries .entry:not([hidden])')?.dataset.path.endsWith('/alpha.txt'));
      input('#quick-filter', '');
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: true, bubbles: true }));
      await waitFor(() => document.querySelector('#search-dialog')?.open);
      record('command-search/open', true);
      document.querySelector('#search-dialog').close();
      click('#grid-view');
      record('list-to-grid', document.querySelector('#entries')?.classList.contains('grid'));
      input('#search', 'alpha');
      await waitFor(() => document.querySelectorAll('#entries .entry').length === 1);
      record('search/input', document.querySelector('#entries .entry')?.dataset.path.endsWith('/alpha.txt'));
      input('#search', '');
      await waitFor(() => document.querySelectorAll('#entries .entry').length > 0);
      const child = [...document.querySelectorAll('#entries .entry')].find((row) => row.dataset.path.endsWith('/child'));
      if (!child) throw new Error('child directory missing');
      child.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
      await waitFor(() => document.querySelector('#breadcrumb .crumb.last')?.textContent === 'child');
      record('navigation/breadcrumb', true);
      if (mode === 'ui-harness') {
        click('#new-menu'); click('#new-folder'); input('#modal-input', 'ui-created'); click('#modal-submit');
        await waitFor(() => [...document.querySelectorAll('#entries .entry')].some((row) => row.dataset.path.endsWith('/ui-created')));
        record('create-folder/feedback', true);
        const created = [...document.querySelectorAll('#entries .entry')].find((row) => row.dataset.path.endsWith('/ui-created'));
        created.dispatchEvent(new MouseEvent('click', { bubbles: true })); document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F2', bubbles: true })); input('#modal-input', 'ui-renamed'); click('#modal-submit');
        await waitFor(() => [...document.querySelectorAll('#entries .entry')].some((row) => row.dataset.path.endsWith('/ui-renamed')));
        record('rename/feedback', true);
        const renamed = [...document.querySelectorAll('#entries .entry')].find((row) => row.dataset.path.endsWith('/ui-renamed'));
        renamed.dispatchEvent(new MouseEvent('click', { bubbles: true })); document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true }));
        await waitFor(() => ![...document.querySelectorAll('#entries .entry')].some((row) => row.dataset.path.endsWith('/ui-renamed')));
        record('trash/feedback', true);
      }
      const language = document.querySelector('#language'); language.value = 'en'; language.dispatchEvent(new Event('change', { bubbles: true }));
      await waitFor(() => document.documentElement.lang === 'en');
      record('language-switch', true);
      report.status = steps.every((step) => step.pass) ? 'PASS' : 'FAIL';
    } catch (error) { record('harness', false, error.message); }
    report.pageState = pageState();
    report.host = { requests: globalThis.__NATIVES_TEST_HOST_REQUESTS__ || [], responses, status: report.pageState.host };
    showReport(report);
  }
  setTimeout(run, 250);
}());
