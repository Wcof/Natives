/**
 * Build path → { before, after } maps for Inspector DiffViewer from Run events
 * and optional live file reads. Pure extraction is testable without window/fs.
 */
import type { FileChange, RunEvent } from '@/lib/assistant-protocol';

export type FileDiffContents = Record<string, { before: string; after: string }>;

/** Pull before/after (or content) from file_changed event payloads. */
export function extractDiffContentsFromEvents(events: RunEvent[]): FileDiffContents {
  const out: FileDiffContents = {};
  for (const event of events) {
    if (event.type !== 'file_changed') continue;
    const path = String(event.payload.path ?? '').trim();
    if (!path) continue;
    const before = String(
      event.payload.before ?? event.payload.old_content ?? event.payload.oldContent ?? '',
    );
    const after = String(
      event.payload.after ??
        event.payload.new_content ??
        event.payload.newContent ??
        event.payload.content ??
        '',
    );
    // Merge: later events for same path win for after; keep before if already set
    const prev = out[path];
    out[path] = {
      before: before || prev?.before || '',
      after: after || prev?.after || '',
    };
  }
  return out;
}

/**
 * Ensure every listed file change has an entry; fill missing "after" via reader.
 * Does not invent before when unknown (empty string = unknown baseline).
 */
export async function hydrateFileDiffContents(options: {
  fileChanges: FileChange[];
  events: RunEvent[];
  /** Optional async file reader (nativesAPI.fs.readFile). */
  readFile?: (path: string) => Promise<string | null>;
  previous?: FileDiffContents;
}): Promise<FileDiffContents> {
  const fromEvents = extractDiffContentsFromEvents(options.events);
  const merged: FileDiffContents = { ...(options.previous ?? {}), ...fromEvents };

  for (const change of options.fileChanges) {
    const path = change.path;
    if (!path) continue;
    const cur = merged[path] ?? { before: '', after: '' };
    if (!cur.after && options.readFile) {
      try {
        const text = await options.readFile(path);
        if (text != null) cur.after = text;
      } catch {
        /* keep empty */
      }
    }
    // If still no before and we have after for a create, before stays ''
    // If modified without before, leave before empty so hunk shows all as add
    // when only after is known — DiffViewer still renders.
    if (!cur.before && !cur.after && change.changeType === 'created') {
      cur.after = cur.after || '';
    }
    merged[path] = cur;
  }
  return merged;
}
