import { execFile } from 'child_process';

/**
 * Shared promisified execFile helper.
 * Consolidates 3 duplicate implementations across search-engine.ts / thumbnail.ts / git.ts.
 */
export function execFilePromise(cmd: string, args: string[], options?: { cwd?: string; timeout?: number }): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { cwd: options?.cwd, timeout: options?.timeout ?? 15000 }, (err: Error | null, stdout: string) => {
      if (err) reject(err);
      else resolve(stdout);
    });
  });
}
