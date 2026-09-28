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
  // 预警联动（审计 F-07/C1 链路终点）：fund 模块经父页转发的异动预警。
  // badge 变色 + 系统通知（通知文案走 _locales，R-I1/F-16）。
  if (message?.type === 'invest:alert') {
    if (chrome.action?.setBadgeText) {
      chrome.action.setBadgeText({ text: message.badgeText || '' });
      chrome.action.setBadgeBackgroundColor({ color: message.isUp ? '#e07b7b' : '#6cc29a' });
    }
    if (chrome.notifications?.create) {
      const title = chrome.i18n.getMessage('investAlertNotifyTitle') || 'Investment Alert';
      const body = chrome.i18n.getMessage('investAlertNotifyBody', [
        String(message.symbol || ''),
        String(message.name || ''),
        String(message.badgeText || ''),
      ]) || `${message.symbol} ${message.badgeText}`;
      chrome.notifications.create({
        type: 'basic',
        iconUrl: 'icons/folder-32.png',
        title,
        message: body.slice(0, 200),
      }, () => void chrome.runtime.lastError);
    }
    return;
  }
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


