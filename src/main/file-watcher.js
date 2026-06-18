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
exports.shouldIgnoreEvent = shouldIgnoreEvent;
exports.getFilePriority = getFilePriority;
exports.startFileWatcher = startFileWatcher;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
// ── Noise Filtering ──
const SIDECAR_PATTERNS = ['-journal', '-shm', '-wal', '.tmp', '.lock'];
/**
 * 判断是否应该忽略此文件变更事件
 */
function shouldIgnoreEvent(event, context) {
    const fileName = path.basename(event.path);
    // 1. 忽略隐藏文件
    if (fileName.startsWith('.'))
        return true;
    // 2. 忽略 SQLite sidecar 文件
    if (SIDECAR_PATTERNS.some((p) => event.path.endsWith(p)))
        return true;
    // 3. 忽略 node_modules 和 .git
    if (event.path.includes('/node_modules/') || event.path.includes('/.git/'))
        return true;
    // 4. 3 秒抑制窗口（用户正在浏览的文件）
    if (context.activeFilePath === event.path && context.lastUserAccessTime) {
        if (Date.now() - context.lastUserAccessTime < 3000)
            return true;
    }
    return false;
}
// ── Priority Queue ──
const HIGH_PRIORITY_EXTS = new Set(['.html', '.htm', '.md', '.mdx']);
const MEDIUM_PRIORITY_EXTS = new Set(['.ts', '.tsx', '.js', '.jsx', '.py', '.rs', '.go']);
/**
 * 获取文件优先级 (1=最高, 3=最低)
 */
function getFilePriority(filePath) {
    const ext = path.extname(filePath).toLowerCase();
    if (HIGH_PRIORITY_EXTS.has(ext))
        return 1;
    if (MEDIUM_PRIORITY_EXTS.has(ext))
        return 2;
    return 3;
}
/**
 * 启动文件监控
 * @param dirPath 监控目录
 * @param cb 回调函数
 * @returns 停止函数
 */
function startFileWatcher(dirPath, cb) {
    const watcher = fs.watch(dirPath, { recursive: true }, (eventType, filename) => {
        if (!filename)
            return;
        const fullPath = path.resolve(dirPath, filename.toString());
        const event = {
            path: fullPath,
            type: eventType === 'rename' ? 'delete' : 'modify',
            timestamp: Date.now(),
        };
        // 如果文件是新增的，尝试区分 create vs modify
        if (eventType === 'rename') {
            try {
                fs.accessSync(fullPath);
                event.type = 'create';
            }
            catch {
                event.type = 'delete';
            }
        }
        const context = {};
        if (!shouldIgnoreEvent(event, context)) {
            cb(event);
        }
    });
    watcher.on('error', () => { });
    return () => {
        try {
            watcher.close();
        }
        catch { /* ignore */ }
    };
}
//# sourceMappingURL=file-watcher.js.map