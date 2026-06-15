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

// ── Directory Watcher ──

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

/** 默认截图目录 */
const SCREENSHOT_DIR = path.join(os.homedir(), 'Desktop');

/**
 * 监控截图目录，检测新截图文件
 * @param onDetected 检测到截图时的回调
 * @returns 停止监控的函数
 */
export function watchScreenshotDir(onDetected: (filePath: string) => void): () => void {
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  const seen = new Set<string>();

  // 初始化已存在的文件
  try {
    const files = fs.readdirSync(SCREENSHOT_DIR);
    for (const f of files) seen.add(f);
  } catch { /* dir may not exist */ }

  const watcher = fs.watch(SCREENSHOT_DIR, (eventType, filename) => {
    if (!filename || eventType !== 'rename') return;

    // 防抖：快速连续事件合并
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      const fullPath = path.join(SCREENSHOT_DIR, filename);
      // 文件必须存在（rename 可能是创建或删除）
      if (!fs.existsSync(fullPath)) return;
      // 已见文件跳过
      if (seen.has(filename)) return;
      // 模式匹配
      if (!detectScreenshotFile(filename)) return;

      seen.add(filename);
      onDetected(fullPath);
    }, 300);
  });

  return () => {
    if (debounceTimer) clearTimeout(debounceTimer);
    watcher.close();
  };
}

// ── Save Annotated Image ──

/**
 * 保存标注后的图片（从 Data URL）
 * @param dataUrl base64 data URL (data:image/png;base64,...)
 * @param targetPath 可选目标路径，默认保存到桌面
 * @returns 保存的文件路径
 */
export function saveAnnotatedImage(dataUrl: string, targetPath?: string): string {
  // 解析 data URL
  const match = dataUrl.match(/^data:image\/(\w+);base64,(.+)$/);
  if (!match) throw new Error('Invalid data URL');

  const ext = match[1] === 'jpeg' ? 'jpg' : match[1]!;
  const buffer = Buffer.from(match[2]!, 'base64');

  const savePath = targetPath || path.join(
    SCREENSHOT_DIR,
    `annotated-${Date.now()}.${ext}`,
  );

  // 确保目录存在
  const dir = path.dirname(savePath);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });

  fs.writeFileSync(savePath, buffer);
  return savePath;
}
