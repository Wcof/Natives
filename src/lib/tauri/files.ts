/**
 * tauri/files — 文件域 facade（ARCH-002）
 *
 * fs / archive / search / git / disk / thumbnail 统一入口；
 * 业务组件只允许经本 facade 访问文件能力；唯一 raw invoke 在 ./core.ts。
 */

import { cmd, convertFileSrc } from './core';
import type { NativesAPI } from './types';

  // File System
export const fs: NativesAPI['fs'] = {
    listDir: (dirPath: string, options?: unknown) => cmd('fs_list_dir', { dirPath, options }),
    listDirDetailed: (dirPath: string, options?: unknown) =>
      cmd('fs_list_dir_detailed', { dirPath, options }),
    readFile: (filePath: string) => cmd('fs_read_file', { filePath }),
    writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) =>
      cmd('fs_write_file_atomic', { filePath, content, expectedMtime }),
    createEntry: async (targetPath: string, type: string) => {
      try {
        // Backend accepts "file" | "dir" | "folder". Tauri maps camelCase → snake_case.
        const entryType = type === 'folder' ? 'folder' : type;
        await cmd('fs_create_entry', { targetPath, entryType });
        return { ok: true };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    renameEntry: async (oldPath: string, newPath: string) => {
      try {
        const path = await cmd<string>('fs_rename_entry', { oldPath, newPath });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    trashEntry: async (filePath: string) => {
      try {
        await cmd('fs_trash_entry', { filePath });
        return { ok: true };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    moveEntry: async (from: string, to: string) => {
      try {
        const path = await cmd<string>('fs_move_entry', { from, to });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    copyEntry: async (from: string, to: string) => {
      try {
        const path = await cmd<string>('fs_copy_entry', { from, to });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    duplicateEntry: async (filePath: string) => {
      try {
        const path = await cmd<string>('fs_duplicate_entry', { filePath });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    stat: (filePath: string) => cmd('fs_stat', { filePath }),
    importFiles: (sourcePaths: string[], destDir: string) =>
      cmd('fs_import_files', { sourcePaths, destDir }),
    recentFiles: (root: string) => cmd('fs_recent_files', { root }),
    saveBlob: (dir: string, name: string, base64Data: string) =>
      cmd('fs_save_blob', { dir, name, base64Data }),
    convertFileSrc: (filePath: string) => convertFileSrc(filePath),
    convertImagePreview: (filePath: string) => cmd('fs_convert_image_preview', { filePath }),
    locate: (query: string, cwd?: string, roots?: string[]) => cmd('fs_locate', { query, cwd, roots }),
    verifyPaths: (candidates: string[]) => cmd('fs_verify_paths', { candidates }),
    trashEntries: async (paths: string[]) => {
      try {
        return await cmd('fs_trash_entries', { paths });
      } catch (e: any) {
        return { ok: false, errors: [{ path: '', error: e?.message || String(e) }] };
      }
    },
    moveEntries: async (paths: string[], destDir: string) => {
      try {
        return await cmd('fs_move_entries', { paths, destDir });
      } catch (e: any) {
        return { ok: false, errors: [{ path: '', error: e?.message || String(e) }] };
      }
    },
    copyEntries: async (paths: string[], destDir: string) => {
      try {
        return await cmd('fs_copy_entries', { paths, destDir });
      } catch (e: any) {
        return { ok: false, errors: [{ path: '', error: e?.message || String(e) }] };
      }
    },
    roots: () => cmd('fs_roots'),
    openWith: (path: string, withApp: 'default' | 'reveal' | 'terminal' | 'editor' = 'default') =>
      cmd('fs_open_with', { path, with: withApp }),
    clipboardCopyFiles: async (paths: string[]) => {
      try {
        return await cmd('fs_clipboard_copy_files', { paths });
      } catch (e: any) {
        return { ok: false, count: 0 };
      }
    },
    clipboardCopyImage: async (filePath: string) => {
      try {
        return await cmd('fs_clipboard_copy_image', { filePath });
      } catch (e: any) {
        return { ok: false };
      }
    },
};

  // Archive
export const archive: NativesAPI['archive'] = {
    list: (archivePath: string) => cmd('archive_list', { archivePath }),
    extract: (archivePath: string, destDir?: string) => cmd('fs_extract_archive', { archivePath, destDir }),
    compress: (paths: string[], destZipPath?: string) => cmd('fs_compress_entries', { paths, destZipPath }),
};

  // Search
export const search: NativesAPI['search'] = {
    grep: (query: string, root: string, options?: unknown) =>
      cmd('search_grep', { query, root, options }),
    files: (query: string, root: string, options?: unknown) =>
      cmd('search_files', { query, root, options }),
    spotlight: (query: string, root: string) =>
      cmd('search_spotlight', { query, root }),
};

  // Git
export const git: NativesAPI['git'] = {
    status: (dirPath: string) => cmd('git_status', { dirPath }),
    diff: (filePath: string) => cmd('git_diff', { filePath }),
    commit: (dirPath: string, message: string) => cmd('git_commit', { dirPath, message }),
    push: (dirPath: string) => cmd('git_push', { dirPath }),
};

  // Disk
export const disk: NativesAPI['disk'] = {
    usage: (dirPath: string) => cmd('disk_usage', { dirPath }),
    systemInfo: () => cmd('disk_system_info'),
    systemMetrics: () => cmd('system_metrics'),
};

  // Thumbnail
export const thumbnail: NativesAPI['thumbnail'] = {
    generate: (filePath: string, width: number) =>
      cmd('thumbnail_generate', { filePath, width }),
};

