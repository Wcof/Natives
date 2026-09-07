// Browser App Module Registry (ADR-0025 D2/D9/D51).
//
// Maps appId → a BUILD-TIME relative UI module path. The catalog is NOT
// allowed to carry scriptUrl / moduleUrl / wasmUrl (D2: no online
// executable download). If the catalog lists an app whose appId is not in
// this map, the surface reports "需要更新 Natives 后才能安装/打开" instead
// of fetching any remote code.
//
// Modules are only imported (lazy) by app.js AFTER the authoritative App
// Store confirms the app is installed (D9: build-time present, 0
// execution while uninstalled).

export const APP_UI_MODULES = Object.freeze({
  'com.natives.app.demo': './apps/demo-ui.js',
  fund: './apps/fund-ui.js',
});

export function resolveUiModule(appId) {
  return typeof appId === 'string' ? APP_UI_MODULES[appId] || null : null;
}

export function isKnownUiModule(appId) {
  return resolveUiModule(appId) !== null;
}
