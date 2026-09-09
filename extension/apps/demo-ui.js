const CHUNK_BYTES = 512 * 1024;

export async function readAllResource(client, appId, packageId) {
  const chunks = [];
  let offset = 0;
  let total;
  let version;
  let format;
  do {
    const response = await client.call('apps:read_resource', { appId, packageId, offset, length: CHUNK_BYTES });
    const binary = atob(response.data || '');
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    if (response.app_id !== appId || response.package_id !== packageId || response.offset !== offset
        || response.length !== bytes.length || !Number.isSafeInteger(response.total_size)
        || response.total_size < offset + bytes.length || (version && response.version !== version)) {
      throw Object.assign(new Error('resource chunk contract mismatch'), { code: 'APP_RESOURCE_CORRUPT' });
    }
    total ??= response.total_size;
    version ??= response.version;
    format ??= response.format;
    if (response.total_size !== total || response.version !== version || response.format !== format) {
      throw Object.assign(new Error('resource changed while reading'), { code: 'APP_RESOURCE_CORRUPT' });
    }
    chunks.push(bytes);
    offset += bytes.length;
    if (!bytes.length && offset < total) throw Object.assign(new Error('resource read made no progress'), { code: 'APP_RESOURCE_CORRUPT' });
  } while (offset < total);
  const bytes = new Uint8Array(total || 0);
  let cursor = 0;
  for (const chunk of chunks) { bytes.set(chunk, cursor); cursor += chunk.length; }
  return { bytes, version, format, totalSize: total || 0 };
}

export function mountApp(ctx) {
  const { app, detail, hostName, stage, t, setToast, createHostClient } = ctx;
  let client;
  let unmounted = false;
  const card = document.createElement('div');
  card.className = 'app-card';
  card.innerHTML = `<h3>${escapeHtml(app.name)}</h3><div class="meta"><span>${escapeHtml(app.kind)}</span><span>${t('demoVersion', '版本')}: ${escapeHtml(app.version)}</span><span>${t('demoHost', 'Host')}: ${escapeHtml(hostName)}</span><span class="badge" id="demo-host-status">${t('demoLoadingResource', '正在读取资源…')}</span></div>`;
  const info = document.createElement('div');
  info.className = 'app-card';
  info.innerHTML = `<h3>${t('demoResourceTitle', '已下载资源内容')}</h3><div class="demo-content"><p class="demo-data-line"></p><div class="demo-image-wrap"></div><button type="button" class="btn" data-action="retry" hidden>${t('retry', '重试')}</button></div>`;
  stage.replaceChildren(card, info);
  const status = card.querySelector('#demo-host-status');
  const dataLine = info.querySelector('.demo-data-line');
  const imageWrap = info.querySelector('.demo-image-wrap');
  const retry = info.querySelector('[data-action="retry"]');
  const setStatus = (ok, text) => { status.textContent = text; status.className = `badge ${ok ? 'ok' : 'err'}`; };

  async function load() {
    retry.hidden = true;
    dataLine.textContent = t('demoLoadingResource', '正在读取资源…');
    imageWrap.replaceChildren();
    try {
      client ??= createHostClient({ onDisconnect: (error, intentional) => {
        if (!unmounted && !intentional) { setStatus(false, t('demoDisconnected', 'Host 已断开')); setToast(error?.message || t('demoDisconnected', 'Host 已断开'), true); }
      } });
      if (!detail?.packages?.length) {
        dataLine.textContent = t('demoNoData', '纯扩展应用，无外部数据包');
        setStatus(true, t('demoHostOk', '在线'));
        return;
      }
      const [data, image] = await Promise.all([
        readAllResource(client, app.app_id, 'demo-data'),
        readAllResource(client, app.app_id, 'demo-image'),
      ]);
      const parsed = JSON.parse(new TextDecoder().decode(data.bytes));
      if (data.version !== app.version || image.version !== app.version || !['png', 'jpeg', 'webp'].includes(image.format)) {
        throw Object.assign(new Error('installed resource version or image format mismatch'), { code: 'APP_RESOURCE_CORRUPT' });
      }
      if (unmounted) return;
      dataLine.textContent = `${t('demoResourceData', '数据载荷')}: ${JSON.stringify(parsed)} (${data.totalSize} B, ${data.format})`;
      const img = document.createElement('img');
      img.src = `data:image/${image.format};base64,${bytesToBase64(image.bytes)}`;
      img.alt = t('demoResourceImageAlt', 'Demo 资源图片');
      img.style.maxWidth = '128px';
      imageWrap.replaceChildren(img);
      setStatus(true, t('demoHostOk', '在线'));
    } catch (error) {
      if (unmounted) return;
      const key = error?.code === 'host_disconnected' ? 'demoDisconnected'
        : /not found/i.test(error?.message || '') ? 'demoPackageMissing' : 'demoPackageCorrupt';
      dataLine.textContent = t(key, key === 'demoPackageMissing' ? '资源包缺失' : key === 'demoDisconnected' ? 'Host 已断开' : '资源包损坏或版本不一致');
      setStatus(false, dataLine.textContent);
      retry.hidden = false;
    }
  }
  retry.onclick = load;
  void load();
  return () => { unmounted = true; try { client?.disconnect(); } catch {} stage.replaceChildren(); };
}

function bytesToBase64(bytes) {
  let out = '';
  for (let offset = 0; offset < bytes.length; offset += 0x8000) out += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  return btoa(out);
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
}
