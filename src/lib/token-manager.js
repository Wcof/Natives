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
exports.generateSessionToken = generateSessionToken;
exports.validateSessionToken = validateSessionToken;
exports.invalidateToken = invalidateToken;
exports.invalidateModuleTokens = invalidateModuleTokens;
exports.invalidateAllTokens = invalidateAllTokens;
exports.rotateStaleTokens = rotateStaleTokens;
exports.getTokenMetrics = getTokenMetrics;
const crypto = __importStar(require("crypto"));
// ── Token Management (P0-3: main-process only, uses Node.js crypto) ──
const TOKEN_TTL_MS = 24 * 60 * 60 * 1000;
const ROTATION_INTERVAL_MS = Math.floor(TOKEN_TTL_MS * 0.7);
let masterSecret;
function getOrCreateSecret() {
    if (masterSecret)
        return masterSecret;
    try {
        const { getDb } = require('../main/database');
        const db = getDb();
        const row = db.prepare("SELECT value FROM settings WHERE key = '_token_master_secret'").get();
        if (row) {
            masterSecret = row.value;
            return masterSecret;
        }
        const newSecret = crypto.randomBytes(32).toString('hex');
        db.prepare("INSERT INTO settings (key, value, updated_at) VALUES ('_token_master_secret', ?, datetime('now'))").run(newSecret);
        masterSecret = newSecret;
        return masterSecret;
    }
    catch {
        masterSecret = crypto.randomBytes(32).toString('hex');
        return masterSecret;
    }
}
const tokenMap = new Map();
function generateSessionToken(moduleId) {
    const nonce = crypto.randomBytes(8).toString('hex');
    const timestamp = Date.now().toString();
    const data = `${moduleId}:${timestamp}:${nonce}`;
    const secret = getOrCreateSecret();
    const token = crypto.createHmac('sha256', secret).update(data).digest('hex');
    tokenMap.set(token, { token, moduleId, createdAt: Date.now() });
    return `${token}:${timestamp}`;
}
function validateSessionToken(token, moduleId) {
    const [hash] = token.split(':');
    if (!hash)
        return false;
    const entry = tokenMap.get(hash);
    if (!entry)
        return false;
    if (Date.now() - entry.createdAt > TOKEN_TTL_MS) {
        tokenMap.delete(hash);
        return false;
    }
    return entry.moduleId === moduleId;
}
function invalidateToken(token) {
    const [hash] = token.split(':');
    if (hash)
        tokenMap.delete(hash);
}
function invalidateModuleTokens(moduleId) {
    for (const [key, value] of tokenMap) {
        if (value.moduleId === moduleId)
            tokenMap.delete(key);
    }
}
function invalidateAllTokens() { tokenMap.clear(); }
function rotateStaleTokens() {
    const now = Date.now();
    let rotated = 0;
    for (const [key, value] of tokenMap) {
        if (now - value.createdAt > ROTATION_INTERVAL_MS) {
            tokenMap.delete(key);
            rotated++;
        }
    }
    return rotated;
}
function getTokenMetrics() {
    let oldest = Infinity;
    for (const value of tokenMap.values()) {
        if (value.createdAt < oldest)
            oldest = value.createdAt;
    }
    return { active: tokenMap.size, oldestMs: oldest === Infinity ? 0 : Date.now() - oldest, ttlMs: TOKEN_TTL_MS };
}
//# sourceMappingURL=token-manager.js.map