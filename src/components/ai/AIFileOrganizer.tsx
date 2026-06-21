'use client';

import { useState, useCallback, useEffect, useRef } from 'react';
import { Package, Edit2, Trash2, Archive, ClipboardList, Ruler, RotateCcw } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface AIProposal {
  id: string;
  action: 'move' | 'rename' | 'delete' | 'archive';
  filePath: string;
  reason: string;
  targetPath?: string;
}

/** Rollback log entry (Natives2: ~/.natives/organize-log/<timestamp>.json) */
interface RollbackEntry {
  from: string;
  to: string;
  action: string;
}

interface RollbackLog {
  dir: string;
  at: number;
  moves: RollbackEntry[];
}

// File type → folder mapping
const TYPE_FOLDERS: Record<string, string> = {
  image: 'Images',
  video: 'Videos',
  audio: 'Audio',
  document: 'Documents',
  code: 'Code',
  archive: 'Archives',
  font: 'Fonts',
  data: 'Data',
};

function getFileCategory(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() || '';
  const imageExts = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico'];
  const videoExts = ['mp4', 'mov', 'avi', 'mkv', 'webm', 'flv'];
  const audioExts = ['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a'];
  const docExts = ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'rtf'];
  const codeExts = ['ts', 'tsx', 'js', 'jsx', 'py', 'rb', 'rs', 'go', 'java', 'c', 'cpp', 'h', 'css', 'html', 'json', 'yaml', 'yml', 'toml', 'md'];
  const archiveExts = ['zip', 'tar', 'gz', 'rar', '7z', 'dmg'];
  const fontExts = ['ttf', 'otf', 'woff', 'woff2'];
  const dataExts = ['csv', 'xml', 'sql', 'db', 'sqlite'];

  if (imageExts.includes(ext)) return 'image';
  if (videoExts.includes(ext)) return 'video';
  if (audioExts.includes(ext)) return 'audio';
  if (docExts.includes(ext)) return 'document';
  if (codeExts.includes(ext)) return 'code';
  if (archiveExts.includes(ext)) return 'archive';
  if (fontExts.includes(ext)) return 'font';
  if (dataExts.includes(ext)) return 'data';
  return 'other';
}

/** Read organize preferences brief file (Natives2: ~/.natives/organize-prefs.md) */
async function readBriefFile(): Promise<string> {
  try {
    const api = window.nativesAPI;
    const result = await api?.fs?.readFile?.('~/.natives/organize-prefs.md') as { content?: string } | undefined;
    return result?.content || '';
  } catch {
    return '';
  }
}

/** Write rollback log for undo support (Natives2: ~/.natives/organize-log/<ms>.json) */
async function writeRollbackLog(dir: string, moves: RollbackEntry[]): Promise<void> {
  if (moves.length === 0) return;
  try {
    const api = window.nativesAPI;
    // Ensure log directory exists
    await api?.fs?.createEntry?.('~/.natives/organize-log', 'directory').catch(() => {});
    const log: RollbackLog = { dir, at: Date.now(), moves };
    const filename = `~/.natives/organize-log/${Date.now()}.json`;
    await api?.fs?.writeFileAtomic?.(filename, JSON.stringify(log, null, 2));
  } catch (err) {
    console.warn('[AIFileOrganizer] Failed to write rollback log:', err);
  }
}

/** Append a preference learned from this organize session (Natives2: preference sedimentation) */
async function appendPreference(preference: string, existingContent?: string): Promise<void> {
  try {
    const api = window.nativesAPI;
    const existing = existingContent ?? await readBriefFile();
    const timestamp = new Date().toISOString().split('T')[0];
    const newEntry = `\n- [${timestamp}] ${preference}`;
    await api?.fs?.writeFileAtomic?.('~/.natives/organize-prefs.md', existing + newEntry);
  } catch { /* ignore */ }
}

export default function AIFileOrganizer() {
  const [proposals, setProposals] = useState<AIProposal[]>([]);
  const [approved, setApproved] = useState<Set<string>>(new Set());
  const [analyzing, setAnalyzing] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [currentDir, setCurrentDir] = useState('~');
  const [locale, setLocale] = useState<Locale>('zh');
  const [analysisMode, setAnalysisMode] = useState<'organize' | 'duplicates' | 'large-files'>('organize');
  const [lastRollback, setLastRollback] = useState<RollbackLog | null>(null);
  const briefContentRef = useRef<string>('');

  const handleAnalyze = useCallback(async () => {
    setAnalyzing(true);
    setProposals([]);
    setApproved(new Set());
    try {
      const api = window.nativesAPI;
      if (!api?.fs?.listDir) return;

      // Read brief file for organize preferences (Natives2)
      const brief = await readBriefFile();
      briefContentRef.current = brief;
      const hasPreferences = brief.trim().length > 0;

      const dir = currentDir || '~';
      const entries = await api.fs.listDir(dir, { sortBy: 'name', sortDir: 'asc', showHidden: false });
      if (!Array.isArray(entries)) return;

      const newProposals: AIProposal[] = [];
      let id = 0;

      if (analysisMode === 'organize') {
        // Group files by type
        const typeGroups: Record<string, typeof entries> = {};
        for (const entry of entries) {
          if (entry.isDir) continue;
          const cat = getFileCategory(entry.name);
          if (cat === 'other') continue;
          if (!typeGroups[cat]) typeGroups[cat] = [];
          typeGroups[cat]!.push(entry);
        }

        // Propose moving groups with 3+ files into type folders
        for (const [cat, files] of Object.entries(typeGroups)) {
          if (files.length >= 3) {
            const folder = TYPE_FOLDERS[cat] || cat;
            for (const file of files) {
              newProposals.push({
                id: `move-${id++}`,
                action: 'move',
                filePath: file.path,
                reason: t(locale, 'aiWorkbench.organizer.proposalReason')
                  .replace('{n}', String(files.length))
                  .replace('{cat}', cat)
                  .replace('{folder}', folder),
                targetPath: `${dir}/${folder}/${file.name}`,
              });
            }
          }
        }

        // Propose deleting .DS_Store and thumbs.db
        for (const entry of entries) {
          if (entry.name === '.DS_Store' || entry.name === 'Thumbs.db' || entry.name === '.thumbs.db') {
            newProposals.push({
              id: `del-${id++}`,
              action: 'delete',
              filePath: entry.path,
              reason: t(locale, 'aiWorkbench.organizer.deleteReason'),
            });
          }
        }
      } else if (analysisMode === 'duplicates') {
        // TASK-013: Find duplicate files by name pattern
        const nameMap = new Map<string, typeof entries>();
        for (const entry of entries) {
          if (entry.isDir) continue;
          // Normalize: remove (1), -copy, etc.
          const base = entry.name.replace(/ \(\d+\)| copy( \d+)?|-copy(-\d+)?/i, '');
          if (!nameMap.has(base)) nameMap.set(base, []);
          nameMap.get(base)!.push(entry);
        }
        for (const [, files] of nameMap) {
          if (files.length > 1) {
            // Keep first, propose deleting/archiving the rest
            for (let i = 1; i < files.length; i++) {
              newProposals.push({
                id: `dup-${id++}`,
                action: 'delete',
                filePath: files[i]!.path,
                reason: `Duplicate of ${files[0]!.name}`,
              });
            }
          }
        }
      } else if (analysisMode === 'large-files') {
        // TASK-013: Find files > 10MB
        for (const entry of entries) {
          if (entry.isDir) continue;
          const size = entry.size || 0;
          if (size > 10 * 1024 * 1024) {
            newProposals.push({
              id: `large-${id++}`,
              action: 'archive',
              filePath: entry.path,
              reason: `Large file (${(size / 1024 / 1024).toFixed(1)}MB) — consider archiving`,
              targetPath: `${dir}/_Large/${entry.name}`,
            });
          }
        }
      }

      setProposals(newProposals);
    } catch (err) {
      console.error('[AIFileOrganizer] Analysis failed:', err);
    } finally {
      setAnalyzing(false);
    }
  }, [currentDir, analysisMode, locale]);

  const handleExecute = useCallback(async () => {
    setExecuting(true);
    try {
      const api = window.nativesAPI;
      const approvedProposals = proposals.filter((p) => approved.has(p.id));
      const rollbackMoves: RollbackEntry[] = [];

      for (const p of approvedProposals) {
        if (p.action === 'move' && p.targetPath) {
          // Create target directory if needed
          const targetDir = p.targetPath.substring(0, p.targetPath.lastIndexOf('/'));
          await api?.fs?.createEntry?.(targetDir, 'directory').catch(() => {});
          await api?.fs?.moveEntry?.(p.filePath, p.targetPath);
          rollbackMoves.push({ from: p.targetPath, to: p.filePath, action: 'move' });
        } else if (p.action === 'delete') {
          await api?.fs?.trashEntry?.(p.filePath);
          rollbackMoves.push({ from: p.filePath, to: '', action: 'trash' });
        } else if (p.action === 'archive' && p.targetPath) {
          const targetDir = p.targetPath.substring(0, p.targetPath.lastIndexOf('/'));
          await api?.fs?.createEntry?.(targetDir, 'directory').catch(() => {});
          await api?.fs?.moveEntry?.(p.filePath, p.targetPath);
          rollbackMoves.push({ from: p.targetPath, to: p.filePath, action: 'move' });
        }
      }

      // Write rollback log + preference sedimentation in parallel (Natives2)
      setLastRollback({ dir: currentDir, at: Date.now(), moves: rollbackMoves });
      const moveCount = approvedProposals.filter(p => p.action === 'move').length;
      const deleteCount = approvedProposals.filter(p => p.action === 'delete').length;
      const writes: Promise<void>[] = [writeRollbackLog(currentDir, rollbackMoves)];
      if (moveCount > 0 || deleteCount > 0) {
        const summary = `Organized ${currentDir}: ${moveCount} moves, ${deleteCount} deletions`;
        writes.push(appendPreference(summary, briefContentRef.current));
      }
      await Promise.all(writes);

      // Clear executed proposals
      setProposals((prev) => prev.filter((p) => !approved.has(p.id)));
      setApproved(new Set());
    } catch (err) {
      console.error('[AIFileOrganizer] Execution failed:', err);
    } finally {
      setExecuting(false);
    }
  }, [proposals, approved, currentDir]);

  const handleUndo = useCallback(() => {
    setProposals([]);
    setApproved(new Set());
  }, []);

  /** Undo the last organize operation using the rollback log (Natives2) */
  const handleUndoLast = useCallback(async () => {
    if (!lastRollback) return;
    setExecuting(true);
    try {
      const api = window.nativesAPI;
      for (const move of lastRollback.moves) {
        if (move.action === 'move' && move.to) {
          // Reverse the move
          const targetDir = move.to.substring(0, move.to.lastIndexOf('/'));
          await api?.fs?.createEntry?.(targetDir, 'directory').catch(() => {});
          await api?.fs?.moveEntry?.(move.from, move.to).catch(() => {});
        }
        // Note: trashed files can't be easily untrashed via API
      }
      setLastRollback(null);
    } catch (err) {
      console.error('[AIFileOrganizer] Undo failed:', err);
    } finally {
      setExecuting(false);
    }
  }, [lastRollback]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px', borderBottom: '1px solid var(--vibe-btn-border)',
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.aiFileOrganizer')}
        </div>
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', marginTop: 2 }}>
          {t(locale, 'aiWorkbench.organizer.description')}
        </div>
        <div style={{ display: 'flex', gap: SPACING.xs, marginTop: 6 }}>
          {(['organize', 'duplicates', 'large-files'] as const).map((mode) => (
            <button
              key={mode}
              className="btn-ghost"
              onClick={() => setAnalysisMode(mode)}
              style={{
                fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
                display: 'inline-flex', alignItems: 'center', gap: 3,
                color: analysisMode === mode ? 'var(--accent)' : 'var(--text-faint)',
                background: analysisMode === mode ? 'var(--accent-soft)' : 'transparent',
              }}
            >
              {mode === 'organize' ? <><Package size={10} /> Organize</> : mode === 'duplicates' ? <><ClipboardList size={10} /> Duplicates</> : <><Ruler size={10} /> Large Files</>}
            </button>
          ))}
        </div>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflow: 'auto', padding: 'var(--space-sm)' }}>
        {proposals.length === 0 ? (
          <div style={{ textAlign: 'center', padding: SPACING.xl }}>
            <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)', marginBottom: SPACING.md }}>
              {analyzing ? t(locale, 'aiWorkbench.organizer.analyzing') : t(locale, 'aiWorkbench.noSuggestions')}
            </div>
            <button
              className="btn btn-primary"
              style={{ width: '100%', fontSize: 'var(--fs-sm)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: SPACING.xs }}
              onClick={handleAnalyze}
              disabled={analyzing}
            >
              {analyzing ? <><MathCurveLoader size={12} strokeWidth={1} particleCount={6} /> {t(locale, 'aiWorkbench.organizer.analyzing')}</> : <><Package size={12} /> {t(locale, 'aiWorkbench.analyze')}</>}
            </button>
          </div>
        ) : (
          <>
            {proposals.map((p) => (
              <div key={p.id} style={{
                padding: 'var(--space-sm)', marginBottom: 6, borderRadius: BORDER_RADIUS.md,
                border: `1px solid ${approved.has(p.id) ? 'var(--accent)' : 'var(--vibe-btn-border)'}`,
                background: approved.has(p.id) ? 'var(--vibe-active-bg)' : 'var(--vibe-toolbar-bg)',
              }}>
                <label style={{ display: 'flex', gap: 'var(--space-sm)', cursor: 'pointer', fontSize: 'var(--fs-sm)' }}>
                  <input
                    type="checkbox"
                    checked={approved.has(p.id)}
                    onChange={() => {
                      setApproved((prev) => {
                        const next = new Set(prev);
                        if (next.has(p.id)) next.delete(p.id);
                        else next.add(p.id);
                        return next;
                      });
                    }}
                  />
                   <div>
                    <div style={{ color: 'var(--text)', display: 'flex', alignItems: 'center', gap: SPACING.xs, flexWrap: 'wrap' }}>
                      <span style={{ display: 'inline-flex', alignItems: 'center', gap: SPACING.xs, color: 'var(--accent)' }}>
                        {p.action === 'move' ? <Package size={12} /> : p.action === 'rename' ? <Edit2 size={12} /> : p.action === 'delete' ? <Trash2 size={12} /> : <Archive size={12} />}
                        <span>{t(locale, `aiWorkbench.organizer.actions.${p.action}`)}</span>
                      </span>
                      <span style={{ fontFamily: 'var(--font-mono)' }}>{p.filePath.split('/').pop()}</span>
                    </div>
                    <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', marginTop: 2 }}>{p.reason}</div>
                    {p.targetPath && (
                      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)', marginTop: 1 }}>
                        → {p.targetPath.split('/').slice(-2).join('/')}
                      </div>
                    )}
                  </div>
                </label>
              </div>
            ))}

              <div style={{ display: 'flex', gap: SPACING.xs, marginTop: 10 }}>
                <button
                  className="btn btn-primary"
                  style={{ flex: 1, fontSize: FONT_SIZE.sm, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: SPACING.xs }}
                  disabled={approved.size === 0 || executing}
                  onClick={handleExecute}
                >
                  {executing ? (
                    <>
                      <MathCurveLoader size={11} strokeWidth={1} particleCount={6} style={{ display: 'inline-block' }} />
                      <span>{t(locale, 'aiWorkbench.execute')}</span>
                    </>
                  ) : (
                    `✓ ${t(locale, 'aiWorkbench.execute')}`
                  )} ({approved.size})
                </button>
                <button
                  className="btn btn-ghost"
                  style={{ fontSize: FONT_SIZE.sm }}
                  onClick={handleUndo}
                >
                  {t(locale, 'aiWorkbench.undoAll')}
                </button>
                {lastRollback && (
                  <button
                    className="btn btn-ghost"
                    style={{ fontSize: FONT_SIZE.sm, display: 'inline-flex', alignItems: 'center', gap: SPACING.xs }}
                    onClick={handleUndoLast}
                    disabled={executing}
                    title={`${lastRollback.moves.length} operations from ${new Date(lastRollback.at).toLocaleTimeString()}`}
                  >
                    <RotateCcw size={11} /> Undo Last
                  </button>
                )}
              </div>
          </>
        )}
      </div>
    </div>
  );
}
