import { execSync } from 'child_process';
import { existsSync } from 'fs';
import { extname } from 'path';

export interface ArchiveEntry {
  name: string;
  size: number;
  isDir: boolean;
}

export interface ArchiveListing {
  entries: ArchiveEntry[];
  truncated: boolean;
  totalSize: number;
}

/**
 * List contents of an archive file (zip, tar, gz, tgz, bz2, xz, 7z, rar).
 * Uses system tools — no runtime dependencies.
 */
export function listArchive(archivePath: string): ArchiveListing {
  if (!existsSync(archivePath)) {
    throw new Error(`Archive not found: ${archivePath}`);
  }

  const ext = extname(archivePath).toLowerCase() || '';
  const entries: ArchiveEntry[] = [];
  let raw = '';

  try {
    if (ext === '.zip' || archivePath.endsWith('.zip')) {
      raw = execSync(`unzip -l "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
    } else if (['.tar', '.tgz', '.gz', '.bz2', '.xz'].some(e => archivePath.endsWith(e)) ||
               ext === '.tar') {
      const flag = archivePath.endsWith('.gz') || archivePath.endsWith('.tgz') ? 'tzf'
        : archivePath.endsWith('.bz2') ? 'tjf'
        : archivePath.endsWith('.xz') ? 'tJf'
        : 'tf';
      raw = execSync(`tar ${flag} "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
    } else if (ext === '.7z') {
      raw = execSync(`7z l "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
    } else if (ext === '.rar') {
      raw = execSync(`unrar l "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
    } else {
      throw new Error(`Unsupported archive format: ${ext}`);
    }
  } catch (err) {
    throw new Error(`Failed to read archive: ${(err as Error).message}`);
  }

  // Parse output based on format
  if (ext === '.zip' || archivePath.endsWith('.zip')) {
    // unzip -l output: size date time name
    const lines = raw.split('\n');
    for (const line of lines) {
      const match = line.match(/^\s*\d+\s+[\d-]+\s+[\d:]+\s+(.+)$/);
      if (match?.[1]) {
        const name = match[1].trim();
        const sizeMatch = line.match(/^\s*(\d+)/);
        entries.push({
          name,
          size: sizeMatch?.[1] ? parseInt(sizeMatch[1], 10) : 0,
          isDir: name.endsWith('/'),
        });
      }
    }
  } else if (['.tar', '.tgz', '.gz', '.bz2', '.xz'].some(e => archivePath.endsWith(e))) {
    // tar output: one path per line
    for (const line of raw.split('\n')) {
      const trimmed = line.trim();
      if (trimmed) {
        entries.push({
          name: trimmed,
          size: 0,
          isDir: trimmed.endsWith('/'),
        });
      }
    }
  } else {
    // 7z/rar: parse table format (skip headers/footers)
    const lines = raw.split('\n');
    let inEntries = false;
    for (const line of lines) {
      if (line.startsWith('----')) { inEntries = !inEntries; continue; }
      if (!inEntries) continue;
      const parts = line.trim().split(/\s+/);
      if (parts.length >= 5) {
        const name = parts.slice(4).join(' ');
        entries.push({
          name,
          size: parseInt(parts[0] || '0', 10) || 0,
          isDir: name.endsWith('/'),
        });
      }
    }
  }

  const totalSize = entries.reduce((sum, e) => sum + e.size, 0);
  const truncated = entries.length > 1000;

  return {
    entries: truncated ? entries.slice(0, 1000) : entries,
    truncated,
    totalSize,
  };
}
