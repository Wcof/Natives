"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.listArchive = listArchive;
const child_process_1 = require("child_process");
const fs_1 = require("fs");
const path_1 = require("path");
/**
 * List contents of an archive file (zip, tar, gz, tgz, bz2, xz, 7z, rar).
 * Uses system tools — no runtime dependencies.
 */
function listArchive(archivePath) {
    if (!(0, fs_1.existsSync)(archivePath)) {
        throw new Error(`Archive not found: ${archivePath}`);
    }
    const ext = (0, path_1.extname)(archivePath).toLowerCase() || '';
    const entries = [];
    let raw = '';
    try {
        if (ext === '.zip' || archivePath.endsWith('.zip')) {
            raw = (0, child_process_1.execSync)(`unzip -l "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
        }
        else if (['.tar', '.tgz', '.gz', '.bz2', '.xz'].some(e => archivePath.endsWith(e)) ||
            ext === '.tar') {
            const flag = archivePath.endsWith('.gz') || archivePath.endsWith('.tgz') ? 'tzf'
                : archivePath.endsWith('.bz2') ? 'tjf'
                    : archivePath.endsWith('.xz') ? 'tJf'
                        : 'tf';
            raw = (0, child_process_1.execSync)(`tar ${flag} "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
        }
        else if (ext === '.7z') {
            raw = (0, child_process_1.execSync)(`7z l "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
        }
        else if (ext === '.rar') {
            raw = (0, child_process_1.execSync)(`unrar l "${archivePath}" 2>/dev/null`, { encoding: 'utf8', timeout: 10000 });
        }
        else {
            throw new Error(`Unsupported archive format: ${ext}`);
        }
    }
    catch (err) {
        throw new Error(`Failed to read archive: ${err.message}`);
    }
    // Parse output based on format
    if (ext === '.zip' || archivePath.endsWith('.zip')) {
        // unzip -l output: size date time name
        const lines = raw.split('\n');
        for (const line of lines) {
            const match = line.match(/^\s*\d+\s+[\d-]+\s+[\d:]+\s+(.+)$/);
            if (match?.[1]) {
                const name = match[1].trim();
                const sizeMatch = line.match(/^\s*(\d+)/);
                entries.push({
                    name,
                    size: sizeMatch?.[1] ? parseInt(sizeMatch[1], 10) : 0,
                    isDir: name.endsWith('/'),
                });
            }
        }
    }
    else if (['.tar', '.tgz', '.gz', '.bz2', '.xz'].some(e => archivePath.endsWith(e))) {
        // tar output: one path per line
        for (const line of raw.split('\n')) {
            const trimmed = line.trim();
            if (trimmed) {
                entries.push({
                    name: trimmed,
                    size: 0,
                    isDir: trimmed.endsWith('/'),
                });
            }
        }
    }
    else {
        // 7z/rar: parse table format (skip headers/footers)
        const lines = raw.split('\n');
        let inEntries = false;
        for (const line of lines) {
            if (line.startsWith('----')) {
                inEntries = !inEntries;
                continue;
            }
            if (!inEntries)
                continue;
            const parts = line.trim().split(/\s+/);
            if (parts.length >= 5) {
                const name = parts.slice(4).join(' ');
                entries.push({
                    name,
                    size: parseInt(parts[0] || '0', 10) || 0,
                    isDir: name.endsWith('/'),
                });
            }
        }
    }
    const totalSize = entries.reduce((sum, e) => sum + e.size, 0);
    const truncated = entries.length > 1000;
    return {
        entries: truncated ? entries.slice(0, 1000) : entries,
        truncated,
        totalSize,
    };
}
//# sourceMappingURL=archive.js.map