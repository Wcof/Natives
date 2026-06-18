"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.validateClipboardData = validateClipboardData;
exports.copyToClipboard = copyToClipboard;
exports.readFromClipboard = readFromClipboard;
// 尝试加载 Electron clipboard，测试环境下可能不可用
let clipboard = null;
try {
    clipboard = require('electron').clipboard;
}
catch {
    // 测试环境下 Electron 不可用
}
/**
 * 验证剪贴板数据是否有效
 */
function validateClipboardData(data) {
    if (!data)
        return false;
    return data.length > 0;
}
/**
 * 复制文本到剪贴板（通过 Electron clipboard API）
 */
function copyToClipboard(text) {
    if (!text)
        return false;
    try {
        if (clipboard) {
            clipboard.writeText(text);
            return true;
        }
        return false;
    }
    catch {
        return false;
    }
}
/**
 * 从剪贴板读取文本
 */
function readFromClipboard() {
    try {
        if (clipboard) {
            return clipboard.readText();
        }
        return '';
    }
    catch {
        return '';
    }
}
//# sourceMappingURL=clipboard.js.map