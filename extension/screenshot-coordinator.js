import {
  computeCaptureTiles,
  validateCaptureLimits,
  MIN_CAPTURE_INTERVAL_MS,
} from './screenshot-engine.js';

let isCapturing = false;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function isProtectedUrl(url) {
  if (!url || typeof url !== 'string') return true;
  const lower = url.toLowerCase();
  return (
    lower.startsWith('chrome://') ||
    lower.startsWith('chrome-extension://') ||
    lower.startsWith('devtools://') ||
    lower.startsWith('edge://') ||
    lower.startsWith('about:') ||
    lower.startsWith('view-source:') ||
    lower.includes('chromewebstore.google.com') ||
    lower.includes('chrome.google.com/webstore')
  );
}

export async function validateTabForCapture(tab) {
  if (!tab || !tab.id) {
    return { ok: false, error: 'captureErrorFailed' };
  }
  const url = tab.url || '';
  if (isProtectedUrl(url)) {
    return { ok: false, error: 'captureErrorProtected' };
  }
  if (url.startsWith('file://')) {
    const isAllowed = await chrome.extension.isAllowedFileSchemeAccess();
    if (!isAllowed) {
      return { ok: false, error: 'captureErrorFileAccess' };
    }
  }
  return { ok: true };
}

export async function handleCaptureTrigger(targetTab) {
  let tab = targetTab;
  if (!tab || !tab.id) {
    const [active] = await chrome.tabs.query({ active: true, currentWindow: true });
    tab = active;
  }

  if (isCapturing) {
    // A capture task is already running
    return;
  }

  const validation = await validateTabForCapture(tab);
  if (!validation.ok) {
    await chrome.tabs.create({
      url: chrome.runtime.getURL(`screenshot.html?error=${encodeURIComponent(validation.error)}`),
      active: true,
    });
    return;
  }

  const sourceTabId = tab.id;
  const sourceWindowId = tab.windowId;
  const sessionId = `s_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;

  isCapturing = true;
  let resultTab = null;
  let streamPort = null;

  const portPromise = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      chrome.runtime.onConnect.removeListener(onConnect);
      reject(new Error('PORT_TIMEOUT'));
    }, 15000);

    function onConnect(port) {
      if (port.name === sessionId) {
        clearTimeout(timeout);
        chrome.runtime.onConnect.removeListener(onConnect);
        resolve(port);
      }
    }
    chrome.runtime.onConnect.addListener(onConnect);
  });

  try {
    // Create inactive result page tab
    resultTab = await chrome.tabs.create({
      url: chrome.runtime.getURL(`screenshot.html?session=${sessionId}`),
      active: false,
    });

    // Inject measurement and scroll helper
    await chrome.scripting.executeScript({
      target: { tabId: sourceTabId },
      files: ['screenshot-content.js'],
    });

    const [{ result: pageMeta }] = await chrome.scripting.executeScript({
      target: { tabId: sourceTabId },
      func: () => window.__nativesScreenshot?.measure(),
    });

    if (!pageMeta) {
      throw new Error('captureErrorFailed');
    }

    const tilePlan = computeCaptureTiles(
      pageMeta.viewportWidth,
      pageMeta.viewportHeight,
      pageMeta.totalWidth,
      pageMeta.totalHeight
    );

    const scale = pageMeta.dpr || 1;
    const limitCheck = validateCaptureLimits(
      tilePlan.tiles.length,
      pageMeta.totalWidth * scale,
      pageMeta.totalHeight * scale
    );

    if (!limitCheck.ok) {
      throw new Error(limitCheck.error);
    }

    streamPort = await portPromise;

    streamPort.postMessage({
      type: 'start',
      title: pageMeta.title,
      totalWidth: pageMeta.totalWidth,
      totalHeight: pageMeta.totalHeight,
      viewportWidth: pageMeta.viewportWidth,
      viewportHeight: pageMeta.viewportHeight,
      dpr: pageMeta.dpr,
      totalTiles: tilePlan.tiles.length,
    });

    for (let i = 0; i < tilePlan.tiles.length; i++) {
      const tile = tilePlan.tiles[i];

      // Confirm source tab is still active in its window
      const [currentActive] = await chrome.tabs.query({ active: true, windowId: sourceWindowId });
      if (!currentActive || currentActive.id !== sourceTabId) {
        throw new Error('captureErrorTabSwitched');
      }
      if (currentActive.url !== pageMeta.url) {
        throw new Error('captureErrorPageNavigated');
      }

      // Scroll to tile coordinate, hiding fixed/sticky elements from 2nd screen onwards
      await chrome.scripting.executeScript({
        target: { tabId: sourceTabId },
        func: (x, y, hideFixed) => window.__nativesScreenshot?.scrollTo(x, y, hideFixed),
        args: [tile.scrollX, tile.scrollY, tile.tileIndex >= 1],
      });

      // Wait at least MIN_CAPTURE_INTERVAL_MS (550ms) to respect quota and lazy-loading
      await sleep(MIN_CAPTURE_INTERVAL_MS);

      // Re-verify source tab is still active right before capture
      const [preCaptureActive] = await chrome.tabs.query({ active: true, windowId: sourceWindowId });
      if (!preCaptureActive || preCaptureActive.id !== sourceTabId) {
        throw new Error('captureErrorTabSwitched');
      }

      // Capture visible tab
      const dataUrl = await chrome.tabs.captureVisibleTab(sourceWindowId, { format: 'png' });

      // Wait for port ack to ensure result page processed the tile before continuing
      await new Promise((resolve, reject) => {
        function onMessage(msg) {
          if (msg.type === 'tile-ack' && msg.tileIndex === tile.tileIndex) {
            streamPort.onMessage.removeListener(onMessage);
            resolve();
          } else if (msg.type === 'error') {
            streamPort.onMessage.removeListener(onMessage);
            reject(new Error(msg.error || 'captureErrorFailed'));
          }
        }
        streamPort.onMessage.addListener(onMessage);

        streamPort.postMessage({
          type: 'tile',
          tileIndex: tile.tileIndex,
          totalTiles: tilePlan.tiles.length,
          dataUrl,
          clipX: tile.clipX,
          clipY: tile.clipY,
          clipWidth: tile.clipWidth,
          clipHeight: tile.clipHeight,
          destX: tile.destX,
          destY: tile.destY,
        });
      });
    }

    // Finished capturing all tiles
    streamPort.postMessage({ type: 'complete' });

    // Switch active tab to the result preview page
    if (resultTab?.id) {
      await chrome.tabs.update(resultTab.id, { active: true });
    }
  } catch (err) {
    const errorKey = err?.message || 'captureErrorFailed';
    if (streamPort) {
      try {
        streamPort.postMessage({ type: 'error', error: errorKey });
      } catch {
        // port may have closed
      }
    }
    if (resultTab?.id) {
      try {
        await chrome.tabs.update(resultTab.id, { active: true });
      } catch {
        // tab may have closed
      }
    }
  } finally {
    // Restore page styles, scrollbars, and position on all exit paths
    try {
      await chrome.scripting.executeScript({
        target: { tabId: sourceTabId },
        func: () => window.__nativesScreenshot?.restore(),
      });
    } catch {
      // tab may have closed or navigated
    }
    isCapturing = false;
  }
}

export function setupScreenshotListeners() {
  chrome.commands.onCommand.addListener((command) => {
    if (command === 'capture-full-page') {
      handleCaptureTrigger();
    }
  });

  chrome.contextMenus.onClicked.addListener((info, tab) => {
    if (info.menuItemId === 'capture-full-page') {
      handleCaptureTrigger(tab);
    }
  });

  chrome.runtime.onInstalled.addListener(() => {
    try {
      chrome.contextMenus.create({
        id: 'capture-full-page',
        title: chrome.i18n.getMessage('captureContextMenu') || '截取完整网页',
        contexts: ['page'],
      });
    } catch {
      // ignore context menu duplicate registration
    }
  });
}
