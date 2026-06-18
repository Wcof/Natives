/**
 * 截图文件名模式匹配（对标 fanbox — 增加 CleanShot/SCR-/截圖）
 */
const SCREENSHOT_PATTERNS = [
  /^Screenshot[\s_]\d{4}/i,
  /^Screen[\s_]Shot[\s_]\d{4}/i,
  /^图片[\s_]\d{4}/,
  /^截图[\s_]\d{4}/,
  /^截圖[\s_]\d{4}/,         // 繁体中文
  /^截屏[\s_]/,              // macOS 中文截屏前缀
  /^\.截屏/,                 // macOS 写盘中间态（点前缀临时文件）
  /^Snipaste_\d{4}/,
  /^微信图片_\d{4}/,
  /^QQ截图\d{4}/,
  /^CleanShot[\s_]/i,        // CleanShot X
  /^SCR-/,                   // SCR 截图工具
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

// ── Directory Watcher（对标 fanbox — 写入稳定性轮询 + 有界去重）──

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { execSync } from 'child_process';

/** 动态检测 macOS 截图保存目录（对标 fanbox 的 defaults read 方式） */
function getScreenshotDir(): string {
  try {
    const out = execSync('defaults read com.apple.screencapture location 2>/dev/null', {
      encoding: 'utf8',
      timeout: 3000,
    }).trim();
    if (out) return out.startsWith('~') ? path.join(os.homedir(), out.slice(1)) : out;
  } catch { /* 未自定义 → 默认桌面 */ }
  return path.join(os.homedir(), 'Desktop');
}

/**
 * 监控截图目录，检测新截图文件
 * - 写入稳定性轮询：stat 每 250ms，连续两次大小一致且 >= 1000 字节才触发
 * - 有界去重 Map：50 条上限，3 秒 TTL
 * @param onDetected 检测到截图时的回调
 * @returns 停止监控的函数
 */
export function watchScreenshotDir(onDetected: (filePath: string) => void): () => void {
  const SCREENSHOT_DIR = getScreenshotDir();
  // 有界去重 Map（对标 fanbox — 50 条上限 + 3s TTL）
  const sentMap = new Map<string, number>();
  const MAX_SENT = 50;
  const TTL_MS = 3000;

  // 初始化已存在的文件（用于跳过启动前的截图）
  const existingFiles = new Set<string>();
  try {
    const files = fs.readdirSync(SCREENSHOT_DIR);
    for (const f of files) existingFiles.add(f);
  } catch { /* dir may not exist */ }

  const watcher = fs.watch(SCREENSHOT_DIR, (eventType, filename) => {
    if (!filename || eventType !== 'rename') return;
    const name = filename.toString();

    // 模式匹配（包含点前缀中间态过滤）
    if (!detectScreenshotFile(name)) return;
    // 跳过启动前已存在的文件
    if (existingFiles.has(name)) return;

    const fp = path.join(SCREENSHOT_DIR, name);

    // ── 写入稳定性轮询（对标 fanbox — Retina 截图几 MB，固定 debounce 不可靠）──
    // 轮询 stat 直到大小连续两次不变且 >= 1000 字节，最多 12 次（~3s）
    const waitStable = (tries: number, lastSize: number): void => {
      fs.stat(fp, (err, st) => {
        if (err || !st.isFile()) return;
        if (st.size >= 1000 && st.size === lastSize) {
          // 大小稳定 = 写盘完成
          // 有界去重：3s 内同一文件不重复触发
          const last = sentMap.get(fp) || 0;
          if (Date.now() - last < TTL_MS) return;
          sentMap.set(fp, Date.now());
          // 50 条上限，淘汰最老的
          if (sentMap.size > MAX_SENT) {
            const oldest = sentMap.keys().next().value;
            if (oldest) sentMap.delete(oldest);
          }
          onDetected(fp);
          return;
        }
        if (tries > 0) setTimeout(() => waitStable(tries - 1, st.size), 250);
      });
    };
    // 首次延迟 350ms（等 macOS 写盘中间态结束），然后开始轮询
    setTimeout(() => waitStable(12, -1), 350);
  });

  return () => {
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
    getScreenshotDir(),
    `annotated-${Date.now()}.${ext}`,
  );

  // 确保目录存在
  const dir = path.dirname(savePath);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });

  fs.writeFileSync(savePath, buffer);
  return savePath;
}
