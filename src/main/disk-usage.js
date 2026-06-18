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
exports.getDiskUsage = getDiskUsage;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const child_process_1 = require("child_process");
/**
 * 获取目录下各项的磁盘用量
 * @param dirPath 目录路径
 * @returns 用量列表（按大小降序）
 */
async function getDiskUsage(dirPath) {
    const stat = await fs.promises.stat(dirPath);
    if (!stat.isDirectory()) {
        throw Object.assign(new Error(`Not a directory: ${dirPath}`), { code: 'ENOTDIR' });
    }
    const entries = await fs.promises.readdir(dirPath);
    const items = [];
    for (const name of entries) {
        if (name.startsWith('.'))
            continue; // 跳过隐藏文件
        const fullPath = path.join(dirPath, name);
        try {
            const entryStat = await fs.promises.stat(fullPath);
            if (entryStat.isDirectory()) {
                // 目录用 du -sk
                const sizeKB = await getDirSizeKB(fullPath);
                const sizeBytes = sizeKB * 1024;
                items.push({
                    name,
                    path: fullPath,
                    isDir: true,
                    size: sizeBytes,
                    sizeFormatted: formatSize(sizeBytes),
                });
            }
            else {
                items.push({
                    name,
                    path: fullPath,
                    isDir: false,
                    size: entryStat.size,
                    sizeFormatted: formatSize(entryStat.size),
                });
            }
        }
        catch {
            // 跳过无权限条目
        }
    }
    // 按大小降序排序
    items.sort((a, b) => b.size - a.size);
    return items;
}
/**
 * 使用 du -sk 获取目录大小（KB）
 */
function getDirSizeKB(dirPath) {
    return new Promise((resolve, reject) => {
        (0, child_process_1.execFile)('du', ['-sk', dirPath], { timeout: 10000 }, (err, stdout) => {
            if (err) {
                reject(err);
                return;
            }
            const match = stdout.match(/^(\d+)/);
            if (match) {
                resolve(parseInt(match[1], 10));
            }
            else {
                resolve(0);
            }
        });
    });
}
/**
 * 格式化大小（人类可读）
 */
function formatSize(bytes) {
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let unitIdx = 0;
    while (size >= 1024 && unitIdx < units.length - 1) {
        size /= 1024;
        unitIdx++;
    }
    return `${size.toFixed(1)} ${units[unitIdx]}`;
}
//# sourceMappingURL=disk-usage.js.map