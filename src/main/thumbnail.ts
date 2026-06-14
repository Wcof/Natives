import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { execFile } from 'child_process';

// ── Image Extension Detection ──

const IMAGE_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp', '.tiff', '.tif', '.heic', '.avif']);
const VIDEO_EXTENSIONS = new Set(['.mp4', '.mov', '.avi', '.mkv', '.webm', '.m4v']);
const THUMBNAILABLE_EXTENSIONS = new Set([...IMAGE_EXTENSIONS, ...VIDEO_EXTENSIONS, '.pdf']);

/**
 * 生成缩略图
 * @param filePath 文件路径
 * @param width 目标宽度（48-1600px）
 * @returns 缩略图 Buffer + Content-Type，不支持的类型返回 null
 */
export async function generateThumb(
  filePath: string,
  width: number,
): Promise<{ buffer: Buffer; contentType: string; cached: boolean } | null> {
  // 宽度限制
  const thumbWidth = Math.max(48, Math.min(1600, width));

  try {
    const stat = await fs.promises.stat(filePath);
    if (!stat.isFile()) return null;
  } catch {
    return null; // 文件不存在
  }

  const ext = path.extname(filePath).toLowerCase();
  if (!THUMBNAILABLE_EXTENSIONS.has(ext)) return null;

  const tmpDir = path.join(os.tmpdir(), 'natives-thumbs');
  await fs.promises.mkdir(tmpDir, { recursive: true });

  const outputPath = path.join(tmpDir, `thumb-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.jpg`);

  try {
    if (IMAGE_EXTENSIONS.has(ext)) {
      // 图片用 sips
      await execFilePromise('sips', [
        '-Z', String(thumbWidth),
        '-s', 'format', 'jpeg',
        filePath,
        '--out', outputPath,
      ]);
    } else {
      // 视频/PDF 用 qlmanage
      await execFilePromise('qlmanage', [
        '-t',
        '-s', String(thumbWidth),
        '-o', tmpDir,
        filePath,
      ]);

      // qlmanage 的输出文件名不同：{basename}.png
      const basename = path.basename(filePath, ext);
      const qlOutput = path.join(tmpDir, `${basename}.png`);
      if (fs.existsSync(qlOutput)) {
        // 用 sips 转成 jpg
        await execFilePromise('sips', [
          '-s', 'format', 'jpeg',
          qlOutput,
          '--out', outputPath,
        ]);
        // 清理 png 临时文件
        try { fs.unlinkSync(qlOutput); } catch { /* ignore */ }
      }
    }

    const buffer = fs.readFileSync(outputPath);
    // 清理输出文件
    try { fs.unlinkSync(outputPath); } catch { /* ignore */ }

    return { buffer, contentType: 'image/jpeg', cached: false };
  } catch {
    // 清理
    try { if (fs.existsSync(outputPath)) fs.unlinkSync(outputPath); } catch { /* ignore */ }
    return null;
  }
}

function execFilePromise(cmd: string, args: string[]): Promise<void> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { timeout: 15000 }, (err) => {
      if (err) reject(err);
      else resolve();
    });
  });
}
