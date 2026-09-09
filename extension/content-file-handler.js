// Minimal content script to detect local Markdown file URLs and request takeover
(() => {
  if (window.top !== window) return;
  const url = location.href;
  if (!/^file:\/\/\/.+/i.test(url)) return;
  const pathname = location.pathname || '';
  if (!/\.(md|markdown|mdx)$/i.test(pathname)) return;
  chrome.runtime.sendMessage({ type: 'open-markdown-file', url });
})();
