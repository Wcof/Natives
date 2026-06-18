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
exports.compareVersions = compareVersions;
exports.parseGithubRelease = parseGithubRelease;
exports.checkForUpdate = checkForUpdate;
exports.muteVersion = muteVersion;
exports.getMutedVersions = getMutedVersions;
/**
 * 比较语义化版本
 * @returns >0 if a > b, <0 if a < b, 0 if equal
 */
function compareVersions(a, b) {
    const pa = a.split('.').map(Number);
    const pb = b.split('.').map(Number);
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
        const na = pa[i] || 0;
        const nb = pb[i] || 0;
        if (na !== nb)
            return na - nb;
    }
    return 0;
}
/**
 * 从 GitHub Release tag 中提取版本号
 */
function parseGithubRelease(tag) {
    const match = tag.match(/(\d+\.\d+\.\d+)/);
    return match ? match[1] : tag;
}
/**
 * 检查更新
 */
async function checkForUpdate(currentVersion, owner, repo) {
    try {
        const res = await fetch(`https://api.github.com/repos/${owner}/${repo}/releases/latest`, {
            headers: { Accept: 'application/vnd.github.v3+json' },
            signal: AbortSignal.timeout(10000),
        });
        if (!res.ok) {
            console.warn(`[UpdateChecker] GitHub API returned ${res.status}: ${res.statusText}`);
            return null;
        }
        const data = await res.json();
        const latestVersion = parseGithubRelease(data.tag_name || '');
        if (latestVersion && compareVersions(latestVersion, currentVersion) > 0) {
            return {
                latestVersion,
                downloadUrl: data.html_url || '',
                releaseNotes: data.body || '',
            };
        }
        return null;
    }
    catch (err) {
        console.warn('[UpdateChecker] checkForUpdate failed:', err.message);
        return null;
    }
}
// ── Muted Versions ──
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const os = __importStar(require("os"));
const MUTE_FILE = path.join(os.homedir(), '.natives', 'muted-versions.json');
function readMutedVersions() {
    try {
        const content = fs.readFileSync(MUTE_FILE, 'utf-8');
        return JSON.parse(content);
    }
    catch {
        return [];
    }
}
function writeMutedVersions(versions) {
    const dir = path.dirname(MUTE_FILE);
    if (!fs.existsSync(dir))
        fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(MUTE_FILE, JSON.stringify(versions, null, 2), 'utf-8');
}
/**
 * 静默指定版本的更新通知
 */
function muteVersion(version) {
    const muted = readMutedVersions();
    if (!muted.includes(version)) {
        muted.push(version);
        writeMutedVersions(muted);
    }
}
/**
 * 获取已静默的版本列表
 */
function getMutedVersions() {
    return readMutedVersions();
}
//# sourceMappingURL=update-checker.js.map