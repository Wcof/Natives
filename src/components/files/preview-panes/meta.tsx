'use client';

/**
 * FilePreview 元信息 pane：保存冲突弹窗 / 文件信息 / Git Diff 视图。
 * 从 FilePreview.tsx 抽出（FIL-004：format views / lifecycle 分责）。
 */

import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import { detectLanguage } from '@/lib/shiki-utils';
import { parseUnifiedDiff } from '@/lib/diff-utils';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import MonacoDiffView from '@/components/assistant/diff/MonacoDiffView';

/** 保存冲突弹窗：磁盘版本比编辑基线新（外部/agent 修改），由用户决定覆盖或暂不保存 */
export function SaveConflictDialog({ open, fileName, locale, onOverwrite, onDismiss }: {
  open: boolean;
  fileName: string;
  locale: Locale;
  onOverwrite: () => void;
  onDismiss: () => void;
}) {
  return (
    <ConfirmDialog
      open={open}
      title={t(locale, 'filePreview.conflictTitle')}
      message={t(locale, 'filePreview.conflictMessage').replace('{name}', fileName)}
      confirmLabel={t(locale, 'filePreview.conflictOverwrite')}
      cancelLabel={t(locale, 'filePreview.conflictKeep')}
      danger
      onConfirm={onOverwrite}
      onCancel={onDismiss}
    />
  );
}

/** 文件信息（info submode） */
export function FileInfo({ entry, locale }: { entry: FileEntry; locale: Locale }) {
  const rows: [string, string][] = [
    [t(locale, 'filePreview.infoName'), entry.name],
    [t(locale, 'filePreview.infoPath'), entry.path],
    [t(locale, 'filePreview.infoType'), entry.kind],
    [t(locale, 'filePreview.infoSize'), `${(entry.size / 1024).toFixed(1)} KB (${entry.size} bytes)`],
    [t(locale, 'filePreview.infoModified'), new Date(entry.mtime).toLocaleString()],
    [t(locale, 'filePreview.infoCreated'), new Date(entry.btime).toLocaleString()],
    [t(locale, 'filePreview.infoHidden'), entry.hidden ? 'Yes' : 'No'],
  ];

  if (entry.isDir) rows.push([t(locale, 'filePreview.infoDirectory'), 'Yes']);
  if (entry.symlink) rows.push([t(locale, 'filePreview.infoSymlink'), entry.symlink]);
  if (entry.projectBadge) rows.push([t(locale, 'filePreview.infoProject'), entry.projectBadge]);

  return (
    <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
      <tbody>
        {rows.map(([key, val]) => (
          <tr key={key} style={{ borderBottom: '1px solid var(--border-subtle)' }}>
            <td style={{ padding: '6px 8px', color: 'var(--text-secondary)', fontWeight: 600, width: 80, verticalAlign: 'top' }}>
              {key}
            </td>
            <td style={{ padding: '6px 8px', color: 'var(--text)', wordBreak: 'break-all' }}>
              {val}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Git Diff 视图（git submode） */
export function GitDiffView({ diff, loading, status, fileName, locale }: {
  diff: string | null;
  loading: boolean;
  status: string | null;
  fileName: string;
  locale: Locale;
}) {
  if (loading) {
    return (
      <div style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: 'var(--text-disabled)',
        fontSize: 12,
        padding: 20,
      }}>
        {t(locale, 'filePreview.gitLoading')}
      </div>
    );
  }

  const noChangesMsg = t(locale, 'fileBrowser.noChanges');
  const notInRepoMsg = t(locale, 'fileBrowser.notInRepo');

  if (!diff || diff === notInRepoMsg || diff === noChangesMsg) {
    const tipText = diff === notInRepoMsg ? notInRepoMsg : noChangesMsg;
    return (
      <div style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 20,
      }}>
        <div style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          gap: 8,
          padding: '16px 24px',
          borderRadius: 8,
          background: 'color-mix(in srgb, var(--text-disabled) 6%, transparent)',
          border: '1px solid color-mix(in srgb, var(--text-disabled) 10%, transparent)',
          color: 'var(--text-secondary)',
          fontSize: 13,
          maxWidth: 260,
          textAlign: 'center',
          lineHeight: 1.5,
        }}>
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.6 }}>
            <circle cx="12" cy="12" r="10" />
            <path d="M12 16v-4" />
            <path d="M12 8h.01" />
          </svg>
          <span>{tipText}</span>
        </div>
      </div>
    );
  }

  const parsed = parseUnifiedDiff(diff);
  if (!parsed) {
    const lines = diff.split('\n');
    return (
      <div style={{ flex: 1 }}>
        {status && (
          <div style={{
            display: 'flex', alignItems: 'center', gap: 6, marginBottom: 10,
            fontSize: 12, color: 'var(--text-secondary)',
          }}>
            <span style={{
              width: 8, height: 8, borderRadius: '50%',
              background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--primary)' : 'var(--warning)',
            }} />
            <span>{status}</span>
          </div>
        )}
        <pre style={{ margin: 0, fontSize: 11, lineHeight: 1.5, fontFamily: 'var(--font-mono)', color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
          {lines.map((line, i) => {
            let color = 'var(--text)';
            const isChanged = line.startsWith('+') && !line.startsWith('+++');
            const isRemoved = line.startsWith('-') && !line.startsWith('---');
            if (isChanged) color = 'var(--primary)';
            else if (isRemoved) color = 'var(--danger)';
            else if (line.startsWith('@@')) color = 'var(--info)';
            else if (line.startsWith('diff') || line.startsWith('index')) color = 'var(--text-disabled)';
            return (
              <div key={i} className={isChanged || isRemoved ? 'anim-clFlash' : ''} style={{ color, background: isChanged ? 'var(--primary-soft)' : isRemoved ? 'color-mix(in srgb, var(--danger) 8%, transparent)' : undefined }}>
                {line || ' '}
              </div>
            );
          })}
        </pre>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      {status && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, padding: '4px 8px',
          fontSize: 11, color: 'var(--text-secondary)',
        }}>
          <span style={{
            width: 8, height: 8, borderRadius: '50%',
            background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--primary)' : 'var(--warning)',
          }} />
          <span>{status}</span>
        </div>
      )}
      <div style={{ flex: 1, minHeight: 0 }}>
        <MonacoDiffView
          original={parsed.original}
          modified={parsed.modified}
          language={detectLanguage(fileName)}
          fileName={fileName}
        />
      </div>
    </div>
  );
}
