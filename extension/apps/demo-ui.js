// Demo App UI (Phase A6 / Gate A6).
//
// Build-time module only: imported lazily by app.js AFTER the App Store
// confirms the demo is installed (ADR-0025 D9). Talks to the demo runtime
// host (`com.natives.app.demo`) through the page-owned port; page closed
// → port closed → host exits (D49).

export function mountApp(ctx) {
  const { app, hostName, stage, t, setToast, createHostClient } = ctx;
  let client;

  const card = document.createElement('div');
  card.className = 'app-card';
  card.innerHTML = `
    <h3>${escapeHtml(app.name)}</h3>
    <div class="meta">
      <span>${escapeHtml(app.kind)}</span>
      <span>${t('demoVersion', '版本')}: ${escapeHtml(app.version)}</span>
      <span>${t('demoHost', 'Host')}: ${escapeHtml(hostName)}</span>
      <span class="badge" id="demo-host-status">…</span>
    </div>`;
  stage.replaceChildren(card);

  const statusBadge = card.querySelector('#demo-host-status');
  function setStatus(ok, text) {
    statusBadge.textContent = text;
    statusBadge.className = `badge ${ok ? 'ok' : 'err'}`;
  }

  const info = document.createElement('div');
  info.className = 'app-card';
  info.innerHTML = `<h3>${t('demoPingTitle', 'Host 通信')}</h3><p></p>`;
  const pingLine = info.querySelector('p');
  pingLine.textContent = t('demoConnecting', '正在连接 demo-host…');
  stage.append(info);

  let unmounted = false;
  (async () => {
    try {
      client = createHostClient({
        onDisconnect: (error, wasIntentional) => {
          if (unmounted || wasIntentional) return;
          setStatus(false, t('demoHostLost', '已断开'));
          setToast(error?.message || t('demoHostLost', 'Host 已断开'), true);
        },
      });
      const [ping, version] = await Promise.all([
        client.call('ping'),
        client.call('version'),
      ]);
      const health = await client.call('health');
      if (unmounted) return;
      setStatus(true, t('demoHostOk', '在线'));
      pingLine.textContent =
        `ping: ${JSON.stringify(ping)} · version: ${JSON.stringify(version)} · health: ${JSON.stringify(health)}`;
    } catch (error) {
      if (unmounted) return;
      setStatus(false, t('demoHostLost', '已断开'));
      setToast(error?.message || String(error), true);
    }
  })();

  // D49: closing the page closes the port → the demo host exits on EOF.
  return () => {
    unmounted = true;
    try { client?.disconnect(); } catch { /* already disconnected */ }
    stage.replaceChildren();
  };
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
