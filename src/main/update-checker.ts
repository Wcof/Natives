/**
 * 比较语义化版本
 * @returns >0 if a > b, <0 if a < b, 0 if equal
 */
export function compareVersions(a: string, b: string): number {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const na = pa[i] || 0;
    const nb = pb[i] || 0;
    if (na !== nb) return na - nb;
  }
  return 0;
}

/**
 * 从 GitHub Release tag 中提取版本号
 */
export function parseGithubRelease(tag: string): string {
  const match = tag.match(/(\d+\.\d+\.\d+)/);
  return match ? match[1]! : tag;
}

/**
 * 检查更新
 */
export async function checkForUpdate(
  currentVersion: string,
  owner: string,
  repo: string,
): Promise<{ latestVersion: string; downloadUrl: string; releaseNotes: string } | null> {
  try {
    const res = await fetch(`https://api.github.com/repos/${owner}/${repo}/releases/latest`, {
      headers: { Accept: 'application/vnd.github.v3+json' },
      signal: AbortSignal.timeout(10000),
    });
    if (!res.ok) {
      console.warn(`[UpdateChecker] GitHub API returned ${res.status}: ${res.statusText}`);
      return null;
    }

    const data = await res.json() as any;
    const latestVersion = parseGithubRelease(data.tag_name || '');
    if (latestVersion && compareVersions(latestVersion, currentVersion) > 0) {
      return {
        latestVersion,
        downloadUrl: data.html_url || '',
        releaseNotes: data.body || '',
      };
    }
    return null;
  } catch (err) {
    console.warn('[UpdateChecker] checkForUpdate failed:', (err as Error).message);
    return null;
  }
}

// ── Muted Versions ──

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

const MUTE_FILE = path.join(os.homedir(), '.natives', 'muted-versions.json');

function readMutedVersions(): string[] {
  try {
    const content = fs.readFileSync(MUTE_FILE, 'utf-8');
    return JSON.parse(content);
  } catch {
    return [];
  }
}

function writeMutedVersions(versions: string[]): void {
  const dir = path.dirname(MUTE_FILE);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(MUTE_FILE, JSON.stringify(versions, null, 2), 'utf-8');
}

/**
 * 静默指定版本的更新通知
 */
export function muteVersion(version: string): void {
  const muted = readMutedVersions();
  if (!muted.includes(version)) {
    muted.push(version);
    writeMutedVersions(muted);
  }
}

/**
 * 获取已静默的版本列表
 */
export function getMutedVersions(): string[] {
  return readMutedVersions();
}
