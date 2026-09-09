import { setupScreenshotListeners } from './screenshot-coordinator.js';

const FILES_URL = chrome.runtime.getURL('files.html');

setupScreenshotListeners();


chrome.runtime.onInstalled.addListener(() => {
  console.log('[Natives] Service Worker initialized successfully.');
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

