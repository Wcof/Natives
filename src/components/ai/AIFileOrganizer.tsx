'use client';

import { useState, useCallback } from 'react';
import { t, type Locale } from '@/i18n';

interface AIProposal {
  id: string;
  action: 'move' | 'rename' | 'delete' | 'archive';
  filePath: string;
  reason: string;
  targetPath?: string;
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

export default function AIFileOrganizer() {
  const [proposals, setProposals] = useState<AIProposal[]>([]);
  const [approved, setApproved] = useState<Set<string>>(new Set());
  const [analyzing, setAnalyzing] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [currentDir, setCurrentDir] = useState('~');
  const [locale, setLocale] = useState<Locale>('zh');
  const [analysisMode, setAnalysisMode] = useState<'organize' | 'duplicates' | 'large-files'>('organize');

  const handleAnalyze = useCallback(async () => {
    setAnalyzing(true);
    setProposals([]);
    setApproved(new Set());
    try {
      const api = window.nativesAPI;
      if (!api?.fs?.listDir) return;

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
  }, [currentDir, analysisMode]);

  const handleExecute = useCallback(async () => {
    setExecuting(true);
    try {
      const api = window.nativesAPI;
      const approvedProposals = proposals.filter((p) => approved.has(p.id));

      for (const p of approvedProposals) {
        if (p.action === 'move' && p.targetPath) {
          // Create target directory if needed
          const targetDir = p.targetPath.substring(0, p.targetPath.lastIndexOf('/'));
          await api?.fs?.createEntry?.(targetDir, 'directory').catch(() => {});
          await api?.fs?.moveEntry?.(p.filePath, p.targetPath);
        } else if (p.action === 'delete') {
          await api?.fs?.trashEntry?.(p.filePath);
        }
      }

      // Clear executed proposals
      setProposals((prev) => prev.filter((p) => !approved.has(p.id)));
      setApproved(new Set());
    } catch (err) {
      console.error('[AIFileOrganizer] Execution failed:', err);
    } finally {
      setExecuting(false);
    }
  }, [proposals, approved]);

  const handleUndo = useCallback(() => {
    setProposals([]);
    setApproved(new Set());
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px', borderBottom: '1px solid var(--border,#262920)',
      }}>
        <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.aiFileOrganizer')}
        </div>
        <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 2 }}>
          {t(locale, 'aiWorkbench.organizer.description')}
        </div>
        <div style={{ display: 'flex', gap: 4, marginTop: 6 }}>
          {(['organize', 'duplicates', 'large-files'] as const).map((mode) => (
            <button
              key={mode}
              className="btn-ghost"
              onClick={() => setAnalysisMode(mode)}
              style={{
                fontSize: 9, padding: '2px 6px', borderRadius: 3,
                color: analysisMode === mode ? 'var(--accent,#cdf24b)' : 'var(--text-faint)',
                background: analysisMode === mode ? 'var(--accent-soft,#cdf24b1f)' : 'transparent',
              }}
            >
              {mode === 'organize' ? '📦 Organize' : mode === 'duplicates' ? '📋 Duplicates' : '📏 Large Files'}
            </button>
          ))}
        </div>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflow: 'auto', padding: 8 }}>
        {proposals.length === 0 ? (
          <div style={{ textAlign: 'center', padding: 20 }}>
            <div style={{ fontSize: 12, color: 'var(--text-faint)', marginBottom: 12 }}>
              {analyzing ? t(locale, 'aiWorkbench.organizer.analyzing') : t(locale, 'aiWorkbench.noSuggestions')}
            </div>
            <button
              className="btn btn-primary"
              style={{ width: '100%', fontSize: 12 }}
              onClick={handleAnalyze}
              disabled={analyzing}
            >
              {analyzing ? `⏳ ${t(locale, 'aiWorkbench.organizer.analyzing')}` : `🔍 ${t(locale, 'aiWorkbench.analyze')}`}
            </button>
          </div>
        ) : (
          <>
            {proposals.map((p) => (
              <div key={p.id} style={{
                padding: 8, marginBottom: 6, borderRadius: 6,
                border: `1px solid ${approved.has(p.id) ? 'var(--accent,#cdf24b)' : 'var(--border,#262920)'}`,
                background: approved.has(p.id) ? 'rgba(205,242,75,0.08)' : 'var(--bg-2,#131410)',
              }}>
                <label style={{ display: 'flex', gap: 8, cursor: 'pointer', fontSize: 12 }}>
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
                    <div style={{ color: 'var(--text)' }}>
                      {p.action === 'move' ? `📦 ${t(locale, 'aiWorkbench.organizer.actions.move')}` : p.action === 'rename' ? `✏️ ${t(locale, 'aiWorkbench.organizer.actions.rename')}` : p.action === 'delete' ? `🗑️ ${t(locale, 'aiWorkbench.organizer.actions.delete')}` : `📁 ${t(locale, 'aiWorkbench.organizer.actions.archive')}`}
                      {' '}<span style={{ fontFamily: 'var(--font-mono)' }}>{p.filePath.split('/').pop()}</span>
                    </div>
                    <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 2 }}>{p.reason}</div>
                    {p.targetPath && (
                      <div style={{ fontSize: 10, color: 'var(--text-dim)', marginTop: 1 }}>
                        → {p.targetPath.split('/').slice(-2).join('/')}
                      </div>
                    )}
                  </div>
                </label>
              </div>
            ))}

              <div style={{ display: 'flex', gap: 6, marginTop: 10 }}>
                <button
                  className="btn btn-primary"
                  style={{ flex: 1, fontSize: 11 }}
                  disabled={approved.size === 0 || executing}
                  onClick={handleExecute}
                >
                  {executing ? '⏳' : `✓ ${t(locale, 'aiWorkbench.execute')}`} ({approved.size})
                </button>
                <button
                  className="btn btn-ghost"
                  style={{ fontSize: 11 }}
                  onClick={handleUndo}
                >
                  {t(locale, 'aiWorkbench.undoAll')}
                </button>
              </div>
          </>
        )}
      </div>
    </div>
  );
}
