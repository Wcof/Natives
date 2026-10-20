// 运行环境检测与能力判定（ADR-0042）

export const isExtension =
  typeof globalThis.chrome !== 'undefined' &&
  Boolean(globalThis.chrome.runtime?.connectNative);

export const isWeb = !isExtension;

export function getBaseEndpoint() {
  if (typeof location !== 'undefined' && location.origin) {
    return location.origin;
  }
  return 'http://127.0.0.1:8770';
}
