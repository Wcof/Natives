"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.walk = walk;
exports.getRecentModifiedFiles = getRecentModifiedFiles;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const os = __importStar(require("os"));
/**
 * 展开路径中的 ~ 为用户主目录
 */
function expandTilde(p) {
    if (p === '~' || p.startsWith('~/') || p.startsWith('~\\')) {
        return path.join(os.homedir(), p.slice(1));
    }
    return p;
}
// ── Constants ──
/** 跳过目录（fanbox 移植） */
const IGNORE_DIRS = new Set([
    'node_modules', '.git', '.svn', '.hg', '.next', '.cache',
    '__pycache__', '.DS_Store', 'dist', 'out', 'build',
    '.idea', '.vscode', '.atomcode', '.mimocode', '.rtk',
]);
/** 最大扫描文件数 */
const MAX_FILES = 30_000;
/** 最大扫描时间（墙钟） */
const MAX_TIME_MS = 3_500;
/** 返回文件数 */
const RESULT_LIMIT = 60;
// ── BFS Walk ──
/**
 * BFS 目录遍历（fanbox 移植）
 * 跳过 IGNORE_DIRS，带文件数/时间上限
 */
function walk(root, onFile, onDir, options) {
    root = expandTilde(root);
    const fileLimit = options?.limit ?? MAX_FILES;
    const deadline = options?.deadline ?? (Date.now() + MAX_TIME_MS);
    let count = 0;
    const queue = [root];
    while (queue.length > 0) {
        // 时间上限检查
        if (Date.now() > deadline)
            break;
        const dir = queue.shift();
        let entries;
        try {
            entries = fs.readdirSync(dir);
        }
        catch {
            continue; // 无权限跳过
        }
        for (const name of entries) {
            if (count >= fileLimit)
                break;
            if (name.startsWith('.') && name !== '.gitkeep')
                continue; // 跳过隐藏文件
            if (IGNORE_DIRS.has(name))
                continue;
            const fullPath = path.join(dir, name);
            let stat;
            try {
                stat = fs.lstatSync(fullPath);
            }
            catch {
                continue;
            }
            if (stat.isDirectory()) {
                queue.push(fullPath);
                onDir?.(fullPath);
            }
            else if (stat.isFile()) {
                count++;
                try {
                    // 对文件使用 stat（解析符号链接）
                    const fileStat = fs.statSync(fullPath);
                    onFile({ path: fullPath, mtime: fileStat.mtimeMs, size: fileStat.size });
                }
                catch {
                    onFile({ path: fullPath, mtime: stat.mtimeMs, size: stat.size });
                }
            }
        }
    }
}
// ── Recent Modified Files ──
/**
 * 获取最近修改的前 N 个文件
 * @param rootPath 根目录
 * @returns 按 mtime 降序排列的文件列表
 */
function getRecentModifiedFiles(rootPath) {
    const files = [];
    walk(rootPath, (file) => files.push(file), undefined, { limit: RESULT_LIMIT * 10 });
    // 按 mtime 降序
    files.sort((a, b) => b.mtime - a.mtime);
    return files.slice(0, RESULT_LIMIT);
}
//# sourceMappingURL=recent-files.js.map