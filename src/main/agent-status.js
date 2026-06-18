"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.detectAgentStatus = detectAgentStatus;
const PROMPT_PATTERNS = [
    /[$%#>]\s*$/, // $, %, #, > at end
    /λ\s/, // Lambda prompt
    /\(.*\)\s*$/, // (env) at end
    /^\s*$/, // empty line (after command)
];
const ERROR_PATTERNS = [
    /^Error:/,
    /^Killed:/,
    /^Segmentation fault/,
    /command not found/,
    /Not a directory/,
    /No such file/,
    /Permission denied/,
];
/**
 * 检测 Agent 状态
 * @param output 终端输出文本
 * @param exitCode 进程退出码
 * @returns Agent 状态
 */
function detectAgentStatus(output, exitCode) {
    // 退出码为 0 → idle
    if (exitCode === 0)
        return 'idle';
    // 非零退出码 → exited
    if (exitCode !== undefined && exitCode > 0)
        return 'exited';
    // 检查错误关键字
    for (const pattern of ERROR_PATTERNS) {
        if (pattern.test(output))
            return 'exited';
    }
    // 检查提示符模式
    for (const pattern of PROMPT_PATTERNS) {
        if (pattern.test(output))
            return 'idle';
    }
    return 'running';
}
//# sourceMappingURL=agent-status.js.map