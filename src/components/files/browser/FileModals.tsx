'use client';

/**
 * FileModals — 文件浏览器重命名 / 新建文件 / 新建文件夹对话框（F3-04 布局子组件，ARCH-002）。
 *
 * 纯展示 + 表单状态路由：Modal 骨架与输入框；状态与确认动作由
 * useFileOperations（renameTarget/newItemTarget…）经父层注入。
 */

import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import Modal from '@/components/ui/Modal';

export interface FileModalsProps {
  locale: Locale;
  // Rename
  renameTarget: FileEntry | null;
  renameValue: string;
  onRenameChange: (value: string) => void;
  onRenameCancel: () => void;
  onRenameConfirm: () => void;
  // New file / folder
  newItemTarget: { parentDir: string; type: 'file' | 'folder' } | null;
  newItemName: string;
  onNewItemChange: (value: string) => void;
  onNewItemCancel: () => void;
  onNewItemConfirm: () => void;
}

export default function FileModals({
  locale,
  renameTarget,
  renameValue,
  onRenameChange,
  onRenameCancel,
  onRenameConfirm,
  newItemTarget,
  newItemName,
  onNewItemChange,
  onNewItemCancel,
  onNewItemConfirm,
}: FileModalsProps) {
  return (
    <>
      {/* Rename dialog */}
      <Modal
        isOpen={!!renameTarget}
        onClose={onRenameCancel}
        title={t(locale, 'fileBrowser.dialogRename')}
        width={340}
      >
        <input
          type="text"
          value={renameValue}
          onChange={(e) => onRenameChange(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && onRenameConfirm()}
          className="input"
          style={{ width: '100%', fontSize: FONT_SIZE.lg }}
          autoFocus
          onFocus={(e) => {
            const v = e.currentTarget.value;
            const dot = v.lastIndexOf('.');
            // Select stem only for files with extension (not dotfiles)
            if (dot > 0 && !renameTarget?.isDir) {
              e.currentTarget.setSelectionRange(0, dot);
            } else {
              e.currentTarget.select();
            }
          }}
        />
        <div style={{ display: 'flex', gap: SPACING.sm, marginTop: 14, justifyContent: 'flex-end' }}>
          <button className="btn btn-ghost" onClick={onRenameCancel}>
            {t(locale, 'common.cancel')}
          </button>
          <button className="btn btn-primary" onClick={onRenameConfirm}>
            {t(locale, 'fileBrowser.dialogRenameBtn')}
          </button>
        </div>
      </Modal>

      {/* New file/folder dialog */}
      <Modal
        isOpen={!!newItemTarget}
        onClose={onNewItemCancel}
        title={
          newItemTarget?.type === 'file'
            ? t(locale, 'fileBrowser.dialogNewFile')
            : t(locale, 'fileBrowser.dialogNewFolder')
        }
        width={340}
      >
        {newItemTarget && (
          <>
            <input
              type="text"
              value={newItemName}
              onChange={(e) => onNewItemChange(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && onNewItemConfirm()}
              placeholder={
                newItemTarget.type === 'file'
                  ? t(locale, 'fileBrowser.placeholderFileName')
                  : t(locale, 'fileBrowser.placeholderFolderName')
              }
              className="input"
              style={{ width: '100%', fontSize: FONT_SIZE.lg }}
              autoFocus
            />
            <div style={{ display: 'flex', gap: SPACING.sm, marginTop: 14, justifyContent: 'flex-end' }}>
              <button className="btn btn-ghost" onClick={onNewItemCancel}>
                {t(locale, 'common.cancel')}
              </button>
              <button className="btn btn-primary" onClick={onNewItemConfirm}>
                {t(locale, 'fileBrowser.dialogCreate')}
              </button>
            </div>
          </>
        )}
      </Modal>
    </>
  );
}
