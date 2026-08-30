let localeToken = 0;
chrome.storage?.onChanged?.addListener((changes, area) => {
  const next = changes['natives-language']?.newValue;
  if (area === 'local' && (next === 'en' || next === 'zh_CN')) applyLocale();
});
async function getLanguage() {
  if (chrome.storage?.local) { try { const value = await chrome.storage.local.get({ 'natives-language': '' }); return value?.['natives-language'] || ''; } catch {} }
  try { return localStorage.getItem('natives-language') || ''; } catch { return ''; }
}
async function setLanguage(language) {
  if (chrome.storage?.local) { try { await chrome.storage.local.set({ 'natives-language': language }); } catch { try { localStorage.setItem('natives-language', language); } catch {} } }
  else { try { localStorage.setItem('natives-language', language); } catch {} }
  await applyLocale();
}
async function applyLocale() {
  const token = ++localeToken;
  const stored = await getLanguage();
  const language = stored === 'en' || stored === 'zh_CN' ? stored : 'zh_CN';
  let messages = {};
  try { const response = await fetch(`_locales/${language}/messages.json`); messages = response.ok ? await response.json() : {}; } catch {}
  if (token !== localeToken) return;
  document.documentElement.lang = language === 'en' ? 'en' : 'zh-CN';
  for (const element of document.querySelectorAll('[data-i18n]')) { const message = messages[element.dataset.i18n]?.message || chrome.i18n.getMessage(element.dataset.i18n); if (message) element.textContent = message; }
  const select = document.querySelector('#language'); if (!select) return; select.value = language; select.setAttribute('aria-label', messages.language?.message || (language === 'en' ? 'Language' : '语言')); select.setAttribute('aria-keyshortcuts', 'Alt+L'); select.onchange = () => setLanguage(select.value === 'en' ? 'en' : 'zh_CN');
}
applyLocale();
document.addEventListener('keydown', (event) => { if (!event.altKey || event.ctrlKey || event.metaKey || event.key.toLowerCase() !== 'l') return; const language = document.querySelector('#language'); if (!language) return; event.preventDefault(); language.focus(); });
