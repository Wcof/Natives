"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseSessionTitle = parseSessionTitle;
exports.parseClaudeSessionFile = parseClaudeSessionFile;
/**
 * 从 JSONL 行中提取会话标题（首条 user/human 消息，截取 100 字符）
 */
function parseSessionTitle(lines) {
    for (const line of lines) {
        try {
            const event = JSON.parse(line);
            // 真实 JSONL 使用 'user' 类型，旧版会话可能使用 'human'
            if ((event.type === 'user' || event.type === 'human') && event.message?.content) {
                const content = event.message.content;
                // content 可以是字符串或数组
                const text = typeof content === 'string'
                    ? content
                    : (Array.isArray(content) ? content[0]?.text ?? '' : '');
                if (text) {
                    return text.length > 100 ? text.slice(0, 100) : text;
                }
            }
        }
        catch {
            continue;
        }
    }
    return 'Untitled';
}
/**
 * 解析 Claude Code 会话文件（JSONL 格式）
 *
 * 真实日志结构验证（2026-06 实测）：
 * - 顶层 type 为 assistant/user/system，没有 tool_use
 * - tool_use 嵌套在 assistant.message.content[] 数组中
 * - 时间戳在顶层 event.timestamp（ISO 8601 格式）
 */
function parseClaudeSessionFile(sessionId, projectPath, lines) {
    const filesModified = new Set();
    const fileTimestamps = {};
    const skillsUsed = new Set();
    const title = parseSessionTitle(lines);
    let startTime = 0;
    for (const line of lines) {
        let event;
        try {
            event = JSON.parse(line);
        }
        catch {
            continue;
        }
        // 时间戳在顶层（已验证格式 "2026-06-11T07:19:42.659Z"）
        const ts = event.timestamp ? Date.parse(event.timestamp) : 0;
        if (ts && (startTime === 0 || ts < startTime))
            startTime = ts;
        // tool_use 嵌套在 assistant.message.content[]
        if (event.type === 'assistant' && Array.isArray(event.message?.content)) {
            for (const block of event.message.content) {
                if (block?.type !== 'tool_use')
                    continue;
                if (['Edit', 'Write', 'NotebookEdit'].includes(block.name)) {
                    const filePath = block.input?.file_path;
                    if (filePath) {
                        filesModified.add(filePath);
                        if (!(filePath in fileTimestamps)) {
                            fileTimestamps[filePath] = ts || 0; // 真实时间戳，替换 eventIndex*100
                        }
                    }
                }
                if (block.name === 'Skill') {
                    const skill = block.input?.skill;
                    if (skill)
                        skillsUsed.add(skill);
                }
            }
        }
    }
    return {
        id: sessionId,
        engine: 'claude',
        projectPath,
        title,
        startTime: startTime || Date.now(), // 真实开始时间，替换裸 Date.now()
        filesModified: [...filesModified],
        fileTimestamps,
        skillsUsed: [...skillsUsed],
    };
}
//# sourceMappingURL=session-scanner.js.map