"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.sendMessage = sendMessage;
exports.broadcastMessage = broadcastMessage;
exports.onMessage = onMessage;
exports.onBroadcast = onBroadcast;
exports.logMessage = logMessage;
exports.getMessageLog = getMessageLog;
exports.clearMessageLog = clearMessageLog;
const bridge_host_1 = require("./bridge-host");
const listeners = new Map(); // moduleId → listeners
const broadcastListeners = new Set(); // global for '*'
// ── Send ──
function sendMessage(from, targetId, channel, payload) {
    // Permission check
    if (!(0, bridge_host_1.checkPermission)(from, 'ipc:send')) {
        throw new Error(`Module '${from}' does not have 'ipc:send' permission`);
    }
    if (!(0, bridge_host_1.checkPermission)(targetId, 'ipc:receive')) {
        throw new Error(`Module '${targetId}' does not have 'ipc:receive' permission`);
    }
    const message = {
        from,
        to: targetId,
        channel,
        payload,
        timestamp: Date.now(),
    };
    logMessage(message);
    // Deliver to target
    const targetListeners = listeners.get(targetId);
    if (targetListeners) {
        for (const listener of targetListeners) {
            listener(message);
        }
    }
}
// ── Broadcast ──
function broadcastMessage(from, channel, payload) {
    if (!(0, bridge_host_1.checkPermission)(from, 'ipc:send')) {
        throw new Error(`Module '${from}' does not have 'ipc:send' permission`);
    }
    const message = {
        from,
        to: '*',
        channel,
        payload,
        timestamp: Date.now(),
    };
    logMessage(message);
    // Deliver to all listeners
    for (const listener of broadcastListeners) {
        listener(message);
    }
    // Also deliver to module-specific listeners
    for (const [, moduleListeners] of listeners) {
        for (const listener of moduleListeners) {
            listener(message);
        }
    }
}
// ── Subscribe ──
function onMessage(moduleId, listener) {
    if (!listeners.has(moduleId)) {
        listeners.set(moduleId, new Set());
    }
    listeners.get(moduleId).add(listener);
    return () => {
        listeners.get(moduleId)?.delete(listener);
    };
}
function onBroadcast(listener) {
    broadcastListeners.add(listener);
    return () => {
        broadcastListeners.delete(listener);
    };
}
// ── Message Logging (audit) ──
const messageLog = [];
const MAX_LOG = 1000;
function logMessage(msg) {
    messageLog.push(msg);
    if (messageLog.length > MAX_LOG) {
        messageLog.shift();
    }
}
function getMessageLog() {
    return [...messageLog];
}
function clearMessageLog() {
    messageLog.length = 0;
}
//# sourceMappingURL=bridge-ipc.js.map