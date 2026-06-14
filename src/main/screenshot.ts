/**
 * 截图文件名模式匹配
 */
const SCREENSHOT_PATTERNS = [
  /^Screenshot[\s_]\d{4}/i,
  /^Screen[\s_]Shot[\s_]\d{4}/i,
  /^图片[\s_]\d{4}/,
  /^截图[\s_]\d{4}/,
  /^Snipaste_\d{4}/,
  /^微信图片_\d{4}/,
  /^QQ截图\d{4}/,
];

/**
 * 检测是否为截图文件
 */
export function detectScreenshotFile(fileName: string): boolean {
  return SCREENSHOT_PATTERNS.some((p) => p.test(fileName));
}

/**
 * 格式化截图显示名称
 */
export function formatDetectedName(fileName: string): string {
  return fileName.replace(/\.\w+$/, '').replace(/[\s_]/g, ' ').toLowerCase();
}
