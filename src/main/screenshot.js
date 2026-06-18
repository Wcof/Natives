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
exports.detectScreenshotFile = detectScreenshotFile;
exports.formatDetectedName = formatDetectedName;
exports.watchScreenshotDir = watchScreenshotDir;
exports.saveAnnotatedImage = saveAnnotatedImage;
/**
 * 截图文件名模式匹配
 */
const SCREENSHOT_PATTERNS = [
    /^Screenshot[\s_]\d{4}/i,
    /^Screen[\s_]Shot[\s_]\d{4}/i,
    /^图片[\s_]\d{4}/,
    /^截图[\s_]\d{4}/,
    /^Snipaste_\d{4}/,
    /^微信图片_\d{4}/,
    /^QQ截图\d{4}/,
];
/**
 * 检测是否为截图文件
 */
function detectScreenshotFile(fileName) {
    return SCREENSHOT_PATTERNS.some((p) => p.test(fileName));
}
/**
 * 格式化截图显示名称
 */
function formatDetectedName(fileName) {
    return fileName.replace(/\.\w+$/, '').replace(/[\s_]/g, ' ').toLowerCase();
}
// ── Directory Watcher ──
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const os = __importStar(require("os"));
/** 默认截图目录 */
const SCREENSHOT_DIR = path.join(os.homedir(), 'Desktop');
/**
 * 监控截图目录，检测新截图文件
 * @param onDetected 检测到截图时的回调
 * @returns 停止监控的函数
 */
function watchScreenshotDir(onDetected) {
    let debounceTimer = null;
    const seen = new Set();
    // 初始化已存在的文件
    try {
        const files = fs.readdirSync(SCREENSHOT_DIR);
        for (const f of files)
            seen.add(f);
    }
    catch { /* dir may not exist */ }
    const watcher = fs.watch(SCREENSHOT_DIR, (eventType, filename) => {
        if (!filename || eventType !== 'rename')
            return;
        // 防抖：快速连续事件合并
        if (debounceTimer)
            clearTimeout(debounceTimer);
        debounceTimer = setTimeout(() => {
            const fullPath = path.join(SCREENSHOT_DIR, filename);
            // 文件必须存在（rename 可能是创建或删除）
            if (!fs.existsSync(fullPath))
                return;
            // 已见文件跳过
            if (seen.has(filename))
                return;
            // 模式匹配
            if (!detectScreenshotFile(filename))
                return;
            seen.add(filename);
            onDetected(fullPath);
        }, 300);
    });
    return () => {
        if (debounceTimer)
            clearTimeout(debounceTimer);
        watcher.close();
    };
}
// ── Save Annotated Image ──
/**
 * 保存标注后的图片（从 Data URL）
 * @param dataUrl base64 data URL (data:image/png;base64,...)
 * @param targetPath 可选目标路径，默认保存到桌面
 * @returns 保存的文件路径
 */
function saveAnnotatedImage(dataUrl, targetPath) {
    // 解析 data URL
    const match = dataUrl.match(/^data:image\/(\w+);base64,(.+)$/);
    if (!match)
        throw new Error('Invalid data URL');
    const ext = match[1] === 'jpeg' ? 'jpg' : match[1];
    const buffer = Buffer.from(match[2], 'base64');
    const savePath = targetPath || path.join(SCREENSHOT_DIR, `annotated-${Date.now()}.${ext}`);
    // 确保目录存在
    const dir = path.dirname(savePath);
    if (!fs.existsSync(dir))
        fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(savePath, buffer);
    return savePath;
}
//# sourceMappingURL=screenshot.js.map