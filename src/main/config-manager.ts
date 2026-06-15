import * as fs from 'fs';
import * as path from 'path';

// Promise chain serialization queue
let cfgChain: Promise<void> = Promise.resolve();

function tmpPath(filePath: string): string {
  const dir = path.dirname(filePath);
  const base = path.basename(filePath);
  return path.join(dir, `.${base}.tmp`);
}

export function readConfig<T>(filePath: string): T {
  if (!fs.existsSync(filePath)) {
    return {} as T;
  }
  try {
    const raw = fs.readFileSync(filePath, 'utf-8');
    return JSON.parse(raw) as T;
  } catch {
    return {} as T;
  }
}

export function updateConfig<T>(
  filePath: string,
  mutator: (config: T) => T,
): Promise<void> {
  cfgChain = cfgChain.then(() => {
    return new Promise<void>((resolve, reject) => {
      const tmp = tmpPath(filePath);
      try {
        // Read current
        const current = readConfig<T>(filePath);
        // Apply mutation
        const next = mutator(current);
        // Write to tmp
        const tmpData = JSON.stringify(next, null, 2);
        fs.writeFileSync(tmp, tmpData, 'utf-8');
        // fsync
        const fd = fs.openSync(tmp, 'r');
        fs.fsyncSync(fd);
        fs.closeSync(fd);
        // Atomic rename
        fs.renameSync(tmp, filePath);
        resolve();
      } catch (err) {
        // Clean up tmp on failure
        try { if (fs.existsSync(tmp)) fs.unlinkSync(tmp); } catch { /* ignore */ }
        reject(err);
      }
    });
  });
  return cfgChain;
}
