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
exports.getEncryptionKey = getEncryptionKey;
exports.encrypt = encrypt;
exports.decrypt = decrypt;
exports.createProfile = createProfile;
exports.deleteProfile = deleteProfile;
exports.listProfiles = listProfiles;
exports.setVariable = setVariable;
exports.getVariables = getVariables;
exports.getProfileById = getProfileById;
exports.getVariablesById = getVariablesById;
exports.setDefaultProfile = setDefaultProfile;
exports.getDefaultProfile = getDefaultProfile;
exports.injectEnv = injectEnv;
exports.injectEnvById = injectEnvById;
const database_1 = require("../main/database");
const crypto = __importStar(require("crypto"));
// ── safeStorage (Electron only, graceful fallback for tests) ──
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let safeStorage = null;
try {
    // Dynamic import: only available in Electron main process
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    safeStorage = require('electron').safeStorage;
}
catch {
    // Not running in Electron (test environment) — use fallback encryption
}
// ── Encryption Key Management ──
const ENCRYPTION_KEY_SETTING = 'env_encryption_key';
function getEncryptionKey() {
    const db = (0, database_1.getDb)();
    const row = db
        .prepare('SELECT value FROM settings WHERE key = ?')
        .get(ENCRYPTION_KEY_SETTING);
    if (row)
        return row.value;
    const key = crypto.randomBytes(32).toString('hex');
    db.prepare('INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime(\'now\'))')
        .run(ENCRYPTION_KEY_SETTING, key);
    return key;
}
// ── Profile CRUD ──
function encrypt(text, encryptionKey) {
    if (safeStorage && safeStorage.isEncryptionAvailable()) {
        const encrypted = safeStorage.encryptString(text);
        return encrypted.toString('base64');
    }
    // ★ P2-2: AES-256-GCM instead of XOR
    const crypto = require('crypto');
    const key = crypto.scryptSync(encryptionKey, 'natives-salt-v1', 32);
    const iv = crypto.randomBytes(16);
    const cipher = crypto.createCipheriv('aes-256-gcm', key, iv);
    let encrypted = cipher.update(text, 'utf8', 'base64');
    encrypted += cipher.final('base64');
    const authTag = cipher.getAuthTag().toString('base64');
    return `${iv.toString('base64')}:${authTag}:${encrypted}`;
}
function decrypt(encoded, encryptionKey) {
    if (safeStorage && safeStorage.isEncryptionAvailable()) {
        const buf = Buffer.from(encoded, 'base64');
        return safeStorage.decryptString(buf);
    }
    // ★ P2-2: Decrypt AES-256-GCM
    const crypto = require('crypto');
    const [ivB64, authTagB64, data] = encoded.split(':');
    if (!ivB64 || !authTagB64 || !data)
        throw new Error('Invalid encrypted format');
    const key = crypto.scryptSync(encryptionKey, 'natives-salt-v1', 32);
    const iv = Buffer.from(ivB64, 'base64');
    const authTag = Buffer.from(authTagB64, 'base64');
    const decipher = crypto.createDecipheriv('aes-256-gcm', key, iv);
    decipher.setAuthTag(authTag);
    let decrypted = decipher.update(data, 'base64', 'utf8');
    decrypted += decipher.final('utf8');
    return decrypted;
}
function createProfile(name) {
    (0, database_1.getDb)()
        .prepare('INSERT INTO env_profiles (name, created_at) VALUES (?, datetime(\'now\'))')
        .run(name);
}
function deleteProfile(name) {
    (0, database_1.getDb)()
        .prepare('DELETE FROM env_profiles WHERE name = ?')
        .run(name);
}
function listProfiles() {
    return (0, database_1.getDb)()
        .prepare('SELECT * FROM env_profiles ORDER BY id')
        .all();
}
// ── Variables ──
async function setVariable(profileName, key, value, encryptionKey) {
    const profile = (0, database_1.getDb)()
        .prepare('SELECT id FROM env_profiles WHERE name = ?')
        .get(profileName);
    if (!profile)
        throw new Error(`Profile '${profileName}' not found`);
    const encrypted = encrypt(value, encryptionKey);
    (0, database_1.getDb)()
        .prepare('INSERT INTO env_variables (profile_id, key, value_encrypted) VALUES (?, ?, ?) ON CONFLICT(profile_id, key) DO UPDATE SET value_encrypted = excluded.value_encrypted')
        .run(profile.id, key, encrypted);
}
async function getVariables(profileName, encryptionKey) {
    const profile = (0, database_1.getDb)()
        .prepare('SELECT id FROM env_profiles WHERE name = ?')
        .get(profileName);
    if (!profile)
        return {};
    const rows = (0, database_1.getDb)()
        .prepare('SELECT key, value_encrypted FROM env_variables WHERE profile_id = ?')
        .all(profile.id);
    const result = {};
    for (const row of rows) {
        try {
            result[row.key] = decrypt(row.value_encrypted, encryptionKey);
        }
        catch {
            result[row.key] = '<decryption error>';
        }
    }
    return result;
}
// ── Profile lookup by id ──
// Look up a profile by its stable numeric id (NOT by name).
// Frontend always passes the profile id; querying by name is fragile because
// the id and name fields are independent. See US25/US26/US29.
function getProfileById(id) {
    const row = (0, database_1.getDb)()
        .prepare('SELECT * FROM env_profiles WHERE id = ? LIMIT 1')
        .get(String(id));
    return row || null;
}
// Resolve variables for a profile by id, decrypting each value.
// Falls back to an empty record if the profile is missing (non-fatal).
async function getVariablesById(profileId, encryptionKey) {
    const profile = getProfileById(profileId);
    if (!profile)
        return {};
    const rows = (0, database_1.getDb)()
        .prepare('SELECT key, value_encrypted FROM env_variables WHERE profile_id = ?')
        .all(profile.id);
    const result = {};
    for (const row of rows) {
        try {
            result[row.key] = decrypt(row.value_encrypted, encryptionKey);
        }
        catch {
            result[row.key] = '<decryption error>';
        }
    }
    return result;
}
// ── Default Profile ──
function setDefaultProfile(name) {
    const db = (0, database_1.getDb)();
    // Reset all profiles to non-default
    db.prepare('UPDATE env_profiles SET is_default = 0').run();
    // Set selected profile as default
    db.prepare('UPDATE env_profiles SET is_default = 1 WHERE name = ?').run(name);
}
function getDefaultProfile() {
    const row = (0, database_1.getDb)()
        .prepare('SELECT * FROM env_profiles WHERE is_default = 1 LIMIT 1')
        .get();
    return row || null;
}
// ── Environment Injection ──
// Inject variables for a profile looked up by NAME (legacy, kept for compatibility).
async function injectEnv(profileName, env, encryptionKey) {
    const vars = await getVariables(profileName, encryptionKey);
    for (const [key, value] of Object.entries(vars)) {
        if (env[key] === undefined) {
            env[key] = value;
        }
    }
}
// Inject variables for a profile looked up by ID.
// This is the correct entry point for terminal sessions, where the frontend
// passes the profile id (see US26 Terminal profile selector).
async function injectEnvById(profileId, env, encryptionKey) {
    const vars = await getVariablesById(profileId, encryptionKey);
    for (const [key, value] of Object.entries(vars)) {
        if (env[key] === undefined) {
            env[key] = value;
        }
    }
}
//# sourceMappingURL=env-injector.js.map