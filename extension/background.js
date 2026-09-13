import { setupScreenshotListeners } from './screenshot-coordinator.js';

const SPACE_URL = chrome.runtime.getURL('space.html');

setupScreenshotListeners();

// 首次拖拽装载扩展后（或者用户安装后首次启动），默认打开空间（首页）：
// 用户安装后期望看到的第一画面是自己的个人主页/空间（space.html），
// 而不是管理性质的 apps.html 应用中心。
chrome.runtime.onInstalled.addListener((details) => {
  console.log('[Natives] Service Worker initialized successfully.');
  if (details.reason === 'install') {
    chrome.tabs.create({ url: SPACE_URL });
  }
});

chrome.action.onClicked.addListener(async () => {
  try {
    const tabs = await chrome.tabs.query({ url: SPACE_URL });
    if (tabs[0]?.id) {
      await chrome.tabs.update(tabs[0].id, { active: true });
      return;
    }
    await chrome.tabs.create({ url: SPACE_URL });
  } catch (err) {
    console.error('[Natives] Failed to open space page:', err);
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

