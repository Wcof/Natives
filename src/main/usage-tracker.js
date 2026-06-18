"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseUsageWindow = parseUsageWindow;
exports.calcRemaining = calcRemaining;
exports.parseClaudeUsage = parseClaudeUsage;
exports.parseCodexUsage = parseCodexUsage;
exports.parseRtkUsage = parseRtkUsage;
// ── Helpers ──
function parseUsageWindow(data) {
    if (!data || typeof data !== 'object')
        return { used: 0, limit: 0, resetAt: 0 };
    return {
        used: data.used || 0,
        limit: data.limit || 0,
        resetAt: data.reset_at || data.resetAt || 0,
    };
}
function calcRemaining(window) {
    if (window.limit === 0)
        return 0;
    return Math.round((1 - window.used / window.limit) * 100);
}
// ── Claude Code Usage ──
function parseClaudeUsage(data) {
    return {
        fiveHourWindow: parseUsageWindow(data?.fiveHourWindow || data?.five_hour_window),
        weeklyQuota: parseUsageWindow(data?.weeklyQuota || data?.weekly_quota),
        localTokens: {
            last5h: data?.localTokens?.last5h || data?.local_tokens?.last_5h || 0,
            today: data?.localTokens?.today || data?.local_tokens?.today || 0,
            thisWeek: data?.localTokens?.thisWeek || data?.local_tokens?.this_week || 0,
        },
    };
}
// ── Codex Usage ──
function parseCodexUsage(data) {
    return {
        fiveHourWindow: parseUsageWindow(data?.fiveHourWindow || data?.five_hour_window),
        planType: data?.planType || data?.plan_type || 'Free',
    };
}
// ── RTK Usage ──
function parseRtkUsage(output) {
    const lines = output.split('\n').filter(Boolean);
    const history = [];
    const commandCounts = new Map();
    let totalSaved = 0;
    let totalCommands = 0;
    for (const line of lines) {
        // Format: command | tokens_saved | timestamp
        const parts = line.split('|').map((s) => s.trim());
        if (parts.length >= 2) {
            const command = parts[0];
            const saved = parseInt(parts[1], 10) || 0;
            const timestamp = parts[2] ? parseInt(parts[2], 10) : Date.now();
            totalSaved += saved;
            totalCommands++;
            history.push({ command, timestamp, tokensSaved: saved });
            const existing = commandCounts.get(command) || { count: 0, totalSaved: 0 };
            existing.count++;
            existing.totalSaved += saved;
            commandCounts.set(command, existing);
        }
    }
    const topCommands = [...commandCounts.entries()]
        .map(([command, stats]) => ({ command, ...stats }))
        .sort((a, b) => b.totalSaved - a.totalSaved)
        .slice(0, 10);
    return { totalSaved, totalCommands, history, topCommands };
}
//# sourceMappingURL=usage-tracker.js.map