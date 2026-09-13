import { setupScreenshotListeners } from './screenshot-coordinator.js';

const FILES_URL = chrome.runtime.getURL('files.html');

setupScreenshotListeners();


// §1.4 前台接续（D02）：首次安装打开应用中心的产品配置入口（含
// "完成 Natives 配置"）；更新不打扰（版本/配置状态由前台页面判定）。
// SW 只处理这一次事件，不持有 Native Port、不轮询。
chrome.runtime.onInstalled.addListener((details) => {
  console.log('[Natives] Service Worker initialized successfully.');
  if (details.reason === 'install') {
    chrome.tabs.create({ url: chrome.runtime.getURL('apps.html') });
  }
});

chrome.action.onClicked.addListener(async () => {
  try {
    const tabs = await chrome.tabs.query({ url: FILES_URL });
    if (tabs[0]?.id) {
      await chrome.tabs.update(tabs[0].id, { active: true });
      return;
    }
    await chrome.tabs.create({ url: FILES_URL });
  } catch (err) {
    console.error('[Natives] Failed to open files page:', err);
  }
});

chrome.runtime.onMessage.addListener((message, sender) => {
  if (message?.type !== 'open-markdown-file') return;
  if (!sender?.tab?.id) return;
  const rawUrl = message.url;
  if (typeof rawUrl !== 'string') return;
  try {
    const parsed = new URL(rawUrl);
    if (parsed.protocol !== 'file:') return;
    if (!/\.(md|markdown|mdx)$/i.test(parsed.pathname)) return;
    const targetUrl = chrome.runtime.getURL(`files.html?open=${encodeURIComponent(rawUrl)}`);
    chrome.tabs.update(sender.tab.id, { url: targetUrl });
  } catch (err) {
    console.error('[Natives] Failed to redirect file URL to files page:', err);
  }
});

