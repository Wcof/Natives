const FILES_URL = chrome.runtime.getURL('files.html');

// The service worker is intentionally stateless: file pages own Native Messaging.
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

