'use client';

import { useState, useCallback, useRef, useEffect } from 'react';
import { Package, Edit2, Trash2, Archive, ClipboardList, FolderOpen, Ruler, RotateCcw } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { t as tr, useLocale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { classifyError } from '@/lib/error-classifier'; // toast+classifyError for catch
import { useToast } from '@/components/ui/Toast';
import { fsApi, hasNativeFiles } from '@/lib/files-api';

/** files-api 契约：fs 不可用（浏览器 dev）时返回 null，调用方用可选链静默降级（与原可选链语义等价） */
function fsOrNull(): ReturnType<typeof fsApi> | null {
  return hasNativeFiles() ? fsApi() : null;
}

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
    const result = await fsOrNull()?.readFile('~/.natives/organize-prefs.md') as { content?: string } | undefined;
    return result?.content || '';
  } catch {
    return '';
  }
}

/** Write rollback log for undo support (Natives2: ~/.natives/organize-log/<ms>.json) */
async function writeRollbackLog(dir: string, moves: RollbackEntry[]): Promise<void> {
  if (moves.length === 0) return;
  try {
    const fs = fsOrNull();
    // Ensure log directory exists
    await fs?.createEntry('~/.natives/organize-log', 'directory').catch(() => {});
    const log: RollbackLog = { dir, at: Date.now(), moves };
    const filename = `~/.natives/organize-log/${Date.now()}.json`;
    await fs?.writeFileAtomic(filename, JSON.stringify(log, null, 2));
  } catch (err) {
    console.warn('[AIFileOrganizer] Failed to write rollback log:', err);
  }
}

/** Append a preference learned from this organize session (Natives2: preference sedimentation) */
async function appendPreference(preference: string, existingContent?: string): Promise<void> {
  try {
    const existing = existingContent ?? await readBriefFile();
    const timestamp = new Date().toISOString().split('T')[0];
    const newEntry = `\n- [${timestamp}] ${preference}`;
    await fsOrNull()?.writeFileAtomic('~/.natives/organize-prefs.md', existing + newEntry);
  } catch { /* ignore */ }
}

export default function AIFileOrganizer() {
  const { toast } = useToast();
  const [proposals, setProposals] = useState<AIProposal[]>([]);
  const [approved, setApproved] = useState<Set<string>>(new Set());
  const [analyzing, setAnalyzing] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [currentDir, setCurrentDir] = useState('~');
  // 旧实现 locale 恒为 'zh'（setLocale 从未被调用），英文用户看到全中文
  const locale = useLocale();
  const t = useCallback((key: string, params?: Record<string, string | number>) => tr(locale, key, params), [locale]);
  const [analysisMode, setAnalysisMode] = useState<'organize' | 'duplicates' | 'large-files'>('organize');
  const [lastRollback, setLastRollback] = useState<RollbackLog | null>(null);
  const briefContentRef = useRef<string>('');

  // T211 (P1-026): on mount, scan ~/.natives/organize-log/ and restore any
  // pending rollback from a previous session so an interrupted organize can
  // still be undone after restart.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const fs = fsOrNull();
        const list = await fs?.listDir('~/.natives/organize-log', { showHidden: false });
        if (!Array.isArray(list) || list.length === 0) return;
        // Latest log file wins (timestamp-suffixed filenames).
        const latest = [...list].sort((a, b) => (b.name ?? '').localeCompare(a.name ?? ''))[0];
        if (!latest?.path) return;
        const raw = await fs?.readFile(latest.path) as { content?: string } | undefined;
        if (!raw?.content || cancelled) return;
        const parsed = JSON.parse(raw.content) as RollbackLog;
        if (parsed && Array.isArray(parsed.moves) && parsed.moves.length > 0) {
          setLastRollback(parsed);
        }
      } catch {
        // Corrupt / unreadable log — ignore; undo simply isn't offered.
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const handleAnalyze = useCallback(async () => {
    setAnalyzing(true);
    setProposals([]);
    setApproved(new Set());
    try {
      // files-api 契约：fs 不可用时直接跳过分析（与原探测语义等价）
      if (!hasNativeFiles()) return;

      // Read brief file for organize preferences (Natives2)
      const brief = await readBriefFile();
      briefContentRef.current = brief;
      // T211 (P1-026): preferences must influence the proposal decision.
      // The brief supports `ignore: <category>` lines (e.g. `ignore: image`),
      // which suppress proposals for that file category.
      const ignoredCategories = new Set(
        brief
          .split('\n')
          .map((line) => line.trim())
          .filter((line) => /^ignore:/i.test(line))
          .map((line) => line.replace(/^ignore:\s*/i, '').trim().toLowerCase())
          .filter((c) => c.length > 0),
      );

      const dir = currentDir || '~';
      const entries = await fsApi().listDir(dir, { sortBy: 'name', sortDir: 'asc', showHidden: false });
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
          if (ignoredCategories.has(cat)) continue; // T211: preference-suppressed
          if (files.length >= 3) {
            const folder = TYPE_FOLDERS[cat] || cat;
            for (const file of files) {
              newProposals.push({
                id: `move-${id++}`,
                action: 'move',
                filePath: file.path,
                reason: t('aiWorkbench.organizer.proposalReason')
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
              reason: t('aiWorkbench.organizer.deleteReason'),
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
                reason: t('aiWorkbench.organizer.duplicateOf', { name: files[0]!.name }),
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
              reason: t('aiWorkbench.organizer.largeFile', { mb: (size / 1024 / 1024).toFixed(1) }),
              targetPath: `${dir}/_Large/${entry.name}`,
            });
          }
        }
      }

      setProposals(newProposals);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setAnalyzing(false);
    }
  }, [currentDir, analysisMode, locale, toast]);

  const handleExecute = useCallback(async () => {
    setExecuting(true);
    try {
      const fs = fsOrNull();
      const approvedProposals = proposals.filter((p) => approved.has(p.id));
      const rollbackMoves: RollbackEntry[] = [];
      // 逐条执行并记录成败：旧实现单条失败即中断整批，且已执行项不从列表移除，
      // 用户无从知道哪些做了哪些没做
      const succeeded = new Set<string>();
      let failed = 0;

      for (const p of approvedProposals) {
        try {
          if (p.action === 'move' && p.targetPath) {
            const targetDir = p.targetPath.substring(0, p.targetPath.lastIndexOf('/'));
            await fs?.createEntry(targetDir, 'directory').catch(() => {});
            await fs?.moveEntry(p.filePath, p.targetPath);
            rollbackMoves.push({ from: p.targetPath, to: p.filePath, action: 'move' });
            succeeded.add(p.id);
          } else if (p.action === 'delete') {
            await fs?.trashEntry(p.filePath);
            rollbackMoves.push({ from: p.filePath, to: '', action: 'trash' });
            succeeded.add(p.id);
          } else if (p.action === 'archive' && p.targetPath) {
            const targetDir = p.targetPath.substring(0, p.targetPath.lastIndexOf('/'));
            await fs?.createEntry(targetDir, 'directory').catch(() => {});
            await fs?.moveEntry(p.filePath, p.targetPath);
            rollbackMoves.push({ from: p.targetPath, to: p.filePath, action: 'move' });
            succeeded.add(p.id);
          }
        } catch {
          failed += 1;
        }
      }

      // Write rollback log + preference sedimentation in parallel (Natives2)
      if (rollbackMoves.length > 0) {
        setLastRollback({ dir: currentDir, at: Date.now(), moves: rollbackMoves });
        const writes: Promise<void>[] = [writeRollbackLog(currentDir, rollbackMoves)];
        const summary = `Organized ${currentDir}: ${rollbackMoves.length} operations`;
        writes.push(appendPreference(summary, briefContentRef.current));
        await Promise.all(writes);
      }

      // 只移除真正执行成功的建议
      setProposals((prev) => prev.filter((p) => !succeeded.has(p.id)));
      setApproved(new Set());
      if (failed > 0) {
        toast(t('aiWorkbench.organizer.partialFailure', { n: failed }), 'error');
      } else if (succeeded.size > 0) {
        toast(t('aiWorkbench.organizer.executeSuccess', { n: succeeded.size }), 'success');
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setExecuting(false);
    }
  }, [proposals, approved, currentDir, toast, t]);

  const handleUndo = useCallback(() => {
    setProposals([]);
    setApproved(new Set());
  }, []);

  /** Undo the last organize operation using the rollback log (Natives2).
   *  T211 (P1-026): failures are reported individually and DO NOT clear
   *  lastRollback — the remaining moves stay retryable. */
  const handleUndoLast = useCallback(async () => {
    if (!lastRollback) return;
    setExecuting(true);
    try {
      const fs = fsOrNull();
      const remaining: RollbackEntry[] = [];
      let succeeded = 0;
      let failed = 0;
      for (const move of lastRollback.moves) {
        try {
          if (move.action === 'move' && move.to) {
            // Reverse the move
            const targetDir = move.to.substring(0, move.to.lastIndexOf('/'));
            await fs?.createEntry(targetDir, 'directory').catch(() => {});
            await fs?.moveEntry(move.from, move.to);
            succeeded += 1;
          } else if (move.action === 'trash') {
            // Trashed files can't be untrashed via API — keep as pending, do
            // not silently claim success.
            remaining.push(move);
          } else {
            remaining.push(move);
          }
        } catch {
          failed += 1;
          // T211: keep failed moves retryable.
          remaining.push(move);
        }
      }
      if (remaining.length > 0) {
        // T211: never clear lastRollback while anything is still pending.
        setLastRollback({ ...lastRollback, moves: remaining });
      } else {
        setLastRollback(null);
      }
      if (failed > 0) {
        toast(t('aiWorkbench.organizer.undoPartialFailure', { failed, pending: remaining.length }), 'error');
      } else if (succeeded > 0) {
        toast(t('aiWorkbench.organizer.undoSuccess', { n: succeeded }), 'success');
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setExecuting(false);
    }
  }, [lastRollback, toast, t]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px', borderBottom: '1px solid var(--border)',
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t('aiWorkbench.aiFileOrganizer')}
        </div>
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', marginTop: 2 }}>
          {t('aiWorkbench.organizer.description')}
        </div>
        <div style={{ display: 'flex', gap: SPACING.xs, marginTop: 6 }}>
          {(['organize', 'duplicates', 'large-files'] as const).map((mode) => (
            <button
              key={mode}
              type="button"
              className="btn-ghost"
              onClick={() => setAnalysisMode(mode)}
              style={{
                fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
                display: 'inline-flex', alignItems: 'center', gap: 3,
                color: analysisMode === mode ? 'var(--primary)' : 'var(--text-disabled)',
                background: analysisMode === mode ? 'var(--primary-soft)' : 'transparent',
              }}
            >
              {mode === 'organize'
                ? <><Package size={10} /> {t('aiWorkbench.organizer.modeOrganize')}</>
                : mode === 'duplicates'
                  ? <><ClipboardList size={10} /> {t('aiWorkbench.organizer.modeDuplicates')}</>
                  : <><Ruler size={10} /> {t('aiWorkbench.organizer.modeLargeFiles')}</>}
            </button>
          ))}
        </div>
        {/* 目标目录：旧实现永远分析 ~，无法整理下载目录等 */}
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, marginTop: 6 }}>
          <span style={{
            fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)',
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, minWidth: 0,
          }} title={currentDir}>
            {currentDir}
          </span>
          <button
            type="button"
            className="btn-ghost"
            onClick={async () => {
              const picked = await window.nativesAPI?.dialog?.pickDirectory?.();
              if (picked) { setCurrentDir(picked); setProposals([]); setApproved(new Set()); }
            }}
            title={t('aiWorkbench.organizer.pickDirectory')}
            aria-label={t('aiWorkbench.organizer.pickDirectory')}
            style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', display: 'inline-flex', alignItems: 'center', gap: 3, color: 'var(--text-secondary)' }}
          >
            <FolderOpen size={11} /> {t('aiWorkbench.organizer.pickDirectory')}
          </button>
        </div>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflow: 'auto', padding: 'var(--space-sm)' }}>
        {proposals.length === 0 ? (
          <div style={{ textAlign: 'center', padding: SPACING.xl }}>
            <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-disabled)', marginBottom: SPACING.md }}>
              {analyzing ? t('aiWorkbench.organizer.analyzing') : t('aiWorkbench.noSuggestions')}
            </div>
            <button
              className="btn btn-primary"
              style={{ width: '100%', fontSize: 'var(--fs-sm)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: SPACING.xs }}
              onClick={handleAnalyze}
              disabled={analyzing}
            >
              {analyzing ? <><MathCurveLoader size={12} strokeWidth={1} particleCount={6} /> {t('aiWorkbench.organizer.analyzing')}</> : <><Package size={12} /> {t('aiWorkbench.analyze')}</>}
            </button>
          </div>
        ) : (
          <>
            {proposals.map((p) => (
              <div key={p.id} style={{
                padding: 'var(--space-sm)', marginBottom: 6, borderRadius: BORDER_RADIUS.md,
                border: `1px solid ${approved.has(p.id) ? 'var(--primary)' : 'var(--border)'}`,
                background: approved.has(p.id) ? 'var(--primary-soft)' : 'var(--surface)',
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
                      <span style={{ display: 'inline-flex', alignItems: 'center', gap: SPACING.xs, color: 'var(--primary)' }}>
                        {p.action === 'move' ? <Package size={12} /> : p.action === 'rename' ? <Edit2 size={12} /> : p.action === 'delete' ? <Trash2 size={12} /> : <Archive size={12} />}
                        <span>{t(`aiWorkbench.organizer.actions.${p.action}`)}</span>
                      </span>
                      <span style={{ fontFamily: 'var(--font-mono)' }}>{p.filePath.split('/').pop()}</span>
                    </div>
                    <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', marginTop: 2 }}>{p.reason}</div>
                    {p.targetPath && (
                      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', marginTop: 1 }}>
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
                      <span>{t('aiWorkbench.execute')}</span>
                    </>
                  ) : (
                    `✓ ${t('aiWorkbench.execute')}`
                  )} ({approved.size})
                </button>
                <button
                  type="button"
                  className="btn btn-ghost"
                  style={{ fontSize: FONT_SIZE.sm }}
                  onClick={handleUndo}
                >
                  {/* 原标签「撤销全部」名不副实：该按钮只清空未执行的建议列表 */}
                  {t('aiWorkbench.organizer.clearProposals')}
                </button>
                {lastRollback && (
                  <button
                    type="button"
                    className="btn btn-ghost"
                    style={{ fontSize: FONT_SIZE.sm, display: 'inline-flex', alignItems: 'center', gap: SPACING.xs }}
                    onClick={handleUndoLast}
                    disabled={executing}
                    title={t('aiWorkbench.organizer.undoLastHint', { n: lastRollback.moves.length })}
                  >
                    <RotateCcw size={11} /> {t('aiWorkbench.organizer.undoLast')}
                  </button>
                )}
              </div>
          </>
        )}
      </div>
    </div>
  );
}
