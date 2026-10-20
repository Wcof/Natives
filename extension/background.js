import { setupScreenshotListeners } from './screenshot-coordinator.js';

const SPACE_URL = chrome.runtime.getURL('space.html');

function isAppSender(sender) {
  if (sender?.id !== chrome.runtime.id) return false;
  try {
    const actual = new URL(sender.url);
    const allowed = new URL(chrome.runtime.getURL('app.html'));
    return actual.protocol === allowed.protocol && actual.host === allowed.host && actual.pathname === allowed.pathname;
  } catch { return false; }
}

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
  // 通用应用通知 Broker（架构 Wave 7 / 审计 A-06）：
  // 处理各内置模块通过通用 natives:notification 发送的通知与 Badge 更新。
  // 不解析任何业务领域专有模型，只消费通用文本与指示色。
  if (message?.type === 'natives:notification-status') {
    if (!isAppSender(sender)) return;
    return chrome.notifications.getPermissionLevel().then(permission=>({permission})).catch(()=>({permission:'unknown'}));
  }
  if (message?.type === 'natives:notification') {
    if (!isAppSender(sender)) return;
    const p = message.payload;
    if (p && typeof p === 'object') {
      if (p.badge?.text !== undefined && chrome.action?.setBadgeText) {
        chrome.action.setBadgeText({ text: String(p.badge.text).slice(0, 16) });
        const toneColors = {
          danger: '#e07b7b',
          warning: '#f0ad4e',
          success: '#6cc29a',
          info: '#49a3b0',
          neutral: '#888888',
        };
        const color = toneColors[p.badge.tone] || '#49a3b0';
        chrome.action.setBadgeBackgroundColor({ color });
      }
      if (p.title && chrome.notifications?.create) {
        return new Promise(resolve=>chrome.notifications.create({
          type: 'basic',
          iconUrl: 'icons/folder-32.png',
          title: String(p.title).slice(0, 64),
          message: String(p.body || '').slice(0, 200),
        }, id=>resolve({delivered:!chrome.runtime.lastError&&Boolean(id)})));
      }
    }
    return Promise.resolve({delivered:false});
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
