#!/usr/bin/env node
/** ADR-0023 installer artifact size gate (macOS pkg / Windows exe). */
import { existsSync, statSync } from 'node:fs';
import { resolve } from 'node:path';

export const DEFAULT_BUDGET = 10 * 1024 * 1024;

export function parseBudget(value) {
  if (value === undefined) return DEFAULT_BUDGET;
  if (!/^\d+$/.test(value)) throw new Error('budget must be a positive integer in bytes');
  const budget = Number(value);
  if (!Number.isSafeInteger(budget) || budget <= 0) throw new Error('budget must be a positive integer in bytes');
  return budget;
}

export function checkInstallerSize(artifact, budget = DEFAULT_BUDGET) {
  if (!artifact) return { ok: false, error: 'artifact path is required', budget };
  if (!Number.isSafeInteger(budget) || budget <= 0) return { ok: false, error: 'budget must be a positive integer in bytes', budget };
  const path = resolve(artifact);
  if (!existsSync(path)) return { ok: false, artifact: path, budget, error: 'artifact does not exist' };
  const bytes = statSync(path).size;
  return { ok: bytes <= budget, artifact: path, bytes, budget, error: bytes <= budget ? undefined : 'artifact exceeds budget' };
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  let summary;
  try {
    summary = checkInstallerSize(process.argv[2], parseBudget(process.argv[3]));
  } catch (error) {
    summary = { ok: false, error: error instanceof Error ? error.message : String(error), budget: DEFAULT_BUDGET };
  }
  console.log(JSON.stringify({ ...summary, budgetMB: summary.budget / (1024 * 1024) }, null, 2));
  process.exitCode = summary.ok ? 0 : 1;
}
