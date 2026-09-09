import { ScreenshotStitcher } from './screenshot-stitcher.js';
import { sanitizeFilename } from './screenshot-engine.js';

function t(key, subs) {
  return chrome.i18n?.getMessage?.(key, subs) || key;
}

let toastTimer = null;
function showToast(message, type = 'info') {
  const toast = document.getElementById('toast');
  if (!toast) return;
  toast.textContent = message;
  toast.className = `toast ${type === 'error' ? 'toast-error' : ''}`;
  toast.hidden = false;
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.hidden = true;
  }, 3500);
}

function showError(errorKey) {
  const banner = document.getElementById('error-banner');
  const messageEl = document.getElementById('error-message');
  const progressContainer = document.getElementById('progress-bar-container');
  const statusEl = document.getElementById('status-text');

  if (messageEl) messageEl.textContent = t(errorKey) || t('captureErrorFailed');
  if (banner) banner.hidden = false;
  if (progressContainer) progressContainer.style.display = 'none';
  if (statusEl) statusEl.textContent = t('captureErrorFailed');
}

function fallbackDownload(blobUrl, filename) {
  const a = document.createElement('a');
  a.href = blobUrl;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
}

async function saveSegmentPng(segment, title, total) {
  const filename = sanitizeFilename(title, segment.index, total);
  if (typeof window.showSaveFilePicker === 'function') {
    try {
      const handle = await window.showSaveFilePicker({
        suggestedName: filename,
        types: [
          {
            description: 'PNG Image',
            accept: { 'image/png': ['.png'] },
          },
        ],
      });
      const writable = await handle.createWritable();
      await writable.write(segment.blob);
      await writable.close();
      return;
    } catch (err) {
      if (err?.name === 'AbortError') return;
    }
  }
  fallbackDownload(segment.blobUrl, filename);
}

document.addEventListener('DOMContentLoaded', () => {
  // Translate data-i18n elements
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    const key = el.getAttribute('data-i18n');
    if (key) {
      el.textContent = t(key);
    }
  });

  const params = new URLSearchParams(location.search);
  const errorParam = params.get('error');
  if (errorParam) {
    showError(errorParam);
    return;
  }

  const sessionId = params.get('session');
  if (!sessionId) {
    showError('captureErrorFailed');
    return;
  }

  const titleEl = document.getElementById('page-title');
  const statusEl = document.getElementById('status-text');
  const progressBar = document.getElementById('progress-bar');
  const progressContainer = document.getElementById('progress-bar-container');
  const container = document.getElementById('screenshot-container');

  const savePngBtn = document.getElementById('save-png');
  const savePdfBtn = document.getElementById('save-pdf');
  const copyImageBtn = document.getElementById('copy-image');

  let stitcher = null;
  let metadata = null;
  let finalizedSegments = [];

  const port = chrome.runtime.connect({ name: sessionId });

  port.onMessage.addListener(async (msg) => {
    try {
      if (msg.type === 'start') {
        metadata = msg;
        if (metadata.title && titleEl) {
          titleEl.textContent = metadata.title;
        }
        if (statusEl) {
          statusEl.textContent = `${t('captureProgress')} (0/${metadata.totalTiles})`;
        }
        if (progressBar) {
          progressBar.style.width = '0%';
        }
        stitcher = new ScreenshotStitcher(metadata);
      } else if (msg.type === 'tile') {
        const current = msg.tileIndex + 1;
        const total = msg.totalTiles;
        if (statusEl) {
          statusEl.textContent = `${t('captureProgress')} (${current}/${total})`;
        }
        if (progressBar) {
          progressBar.style.width = `${Math.round((current / total) * 100)}%`;
        }

        const img = new Image();
        await new Promise((resolve, reject) => {
          img.onload = resolve;
          img.onerror = reject;
          img.src = msg.dataUrl;
        });

        await stitcher.addTile(msg, img);
        port.postMessage({ type: 'tile-ack', tileIndex: msg.tileIndex });
      } else if (msg.type === 'complete') {
        if (statusEl) {
          statusEl.textContent = t('captureStitching');
        }

        finalizedSegments = await stitcher.finalize();
        container.innerHTML = '';

        finalizedSegments.forEach((segment) => {
          const segDiv = document.createElement('div');
          segDiv.className = 'screenshot-segment';

          if (finalizedSegments.length > 1) {
            const label = document.createElement('div');
            label.className = 'segment-label';
            label.textContent = t('captureSegment', [String(segment.index), String(segment.total)]);
            segDiv.appendChild(label);
          }

          const img = document.createElement('img');
          img.src = segment.blobUrl;
          img.alt = metadata?.title || t('captureTitle');
          segDiv.appendChild(img);

          container.appendChild(segDiv);
        });

        if (statusEl) {
          statusEl.textContent = t('captureComplete');
        }
        if (progressContainer) {
          progressContainer.style.display = 'none';
        }

        if (savePngBtn) savePngBtn.disabled = false;
        if (savePdfBtn) savePdfBtn.disabled = false;
        if (copyImageBtn) copyImageBtn.disabled = false;
      } else if (msg.type === 'error') {
        showError(msg.error);
      }
    } catch {
      showError('captureErrorFailed');
    }
  });

  port.onDisconnect.addListener(() => {
    if (!finalizedSegments.length && !document.getElementById('error-banner').hidden === false) {
      // If disconnected prematurely without finishing
      showError('captureErrorTabSwitched');
    }
  });

  savePdfBtn?.addEventListener('click', () => {
    window.print();
  });

  savePngBtn?.addEventListener('click', async () => {
    if (!finalizedSegments.length) return;
    for (const seg of finalizedSegments) {
      await saveSegmentPng(seg, metadata?.title, finalizedSegments.length);
    }
  });

  copyImageBtn?.addEventListener('click', async () => {
    if (!finalizedSegments.length) return;
    try {
      const primaryBlob = finalizedSegments[0].blob;
      if (!primaryBlob) throw new Error('NO_BLOB');
      await navigator.clipboard.write([
        new ClipboardItem({ 'image/png': primaryBlob }),
      ]);
      showToast(t('captureCopied'));
    } catch {
      showToast(t('captureCopyFailed'), 'error');
    }
  });
});
