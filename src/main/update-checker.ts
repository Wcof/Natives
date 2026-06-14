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
    if (!res.ok) return null;

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
  } catch {
    return null;
  }
}
