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
exports.checkPackageJson = checkPackageJson;
exports.checkGitStatus = checkGitStatus;
exports.checkChangelog = checkChangelog;
exports.checkGhCli = checkGhCli;
exports.inspectProject = inspectProject;
exports.getCommandSequence = getCommandSequence;
exports.prepareRelease = prepareRelease;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const child_process_1 = require("child_process");
// ── Checks ──
/**
 * 检查 package.json 是否存在且有效
 */
async function checkPackageJson(dirPath) {
    const pkgPath = path.join(dirPath, 'package.json');
    try {
        const content = await fs.promises.readFile(pkgPath, 'utf-8');
        const pkg = JSON.parse(content);
        if (!pkg.version)
            return { ok: false, message: 'package.json missing version field' };
        return { ok: true, message: `Version ${pkg.version}` };
    }
    catch (err) {
        return { ok: false, message: err.code === 'ENOENT' ? 'package.json not found' : 'Invalid package.json' };
    }
}
/**
 * 检查 git 状态是否干净
 */
async function checkGitStatus(dirPath) {
    try {
        const stdout = await execPromise('git', ['status', '--porcelain'], dirPath);
        if (stdout.trim()) {
            const lines = stdout.trim().split('\n');
            return { ok: false, message: `${lines.length} uncommitted change(s)` };
        }
        return { ok: true, message: 'Working tree clean' };
    }
    catch {
        return { ok: false, message: 'Not a git repository' };
    }
}
/**
 * 检查 CHANGELOG.md 是否存在
 */
async function checkChangelog(dirPath) {
    const clPath = path.join(dirPath, 'CHANGELOG.md');
    try {
        await fs.promises.access(clPath);
        return { ok: true, message: 'CHANGELOG.md found' };
    }
    catch {
        return { ok: false, message: 'CHANGELOG.md not found' };
    }
}
/**
 * 检查 gh CLI 是否可用
 */
async function checkGhCli() {
    try {
        const stdout = await execPromise('gh', ['--version'], '/tmp');
        if (stdout)
            return { ok: true, message: 'gh CLI available' };
        return { ok: false, message: 'gh CLI not found' };
    }
    catch {
        return { ok: false, message: 'gh CLI not found' };
    }
}
function execPromise(cmd, args, cwd) {
    return new Promise((resolve, reject) => {
        (0, child_process_1.execFile)(cmd, args, { cwd, timeout: 5000 }, (err, stdout) => {
            if (err)
                reject(err);
            else
                resolve(stdout);
        });
    });
}
/**
 * 检查项目发布就绪状态
 */
async function inspectProject(projectPath) {
    const pkgCheck = await checkPackageJson(projectPath);
    const gitCheck = await checkGitStatus(projectPath);
    const clCheck = await checkChangelog(projectPath);
    const ghCheck = await checkGhCli();
    let currentVersion = '0.0.0';
    try {
        const content = await fs.promises.readFile(path.join(projectPath, 'package.json'), 'utf-8');
        const pkg = JSON.parse(content);
        if (pkg.version)
            currentVersion = pkg.version;
    }
    catch { /* ignore */ }
    return {
        currentVersion,
        checks: [
            { name: 'package.json', ...pkgCheck },
            { name: 'git status', ...gitCheck },
            { name: 'CHANGELOG.md', ...clCheck },
            { name: 'gh CLI', ...ghCheck },
        ],
    };
}
/**
 * 生成发布命令序列
 */
function getCommandSequence(projectPath, version) {
    return [
        { label: 'Update version', command: `npm version ${version} --no-git-tag-version` },
        { label: 'Git commit', command: `git add -A && git commit -m "release: v${version}"` },
        { label: 'Git tag', command: `git tag v${version}` },
        { label: 'Push', command: 'git push && git push --tags' },
    ];
}
/**
 * 准备发布（更新版本号 + CHANGELOG）
 */
async function prepareRelease(projectPath, version) {
    // 更新 package.json 版本
    const pkgPath = path.join(projectPath, 'package.json');
    const content = await fs.promises.readFile(pkgPath, 'utf-8');
    const pkg = JSON.parse(content);
    pkg.version = version;
    await fs.promises.writeFile(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf-8');
    // 更新 CHANGELOG.md 的 Unreleased 段落
    const clPath = path.join(projectPath, 'CHANGELOG.md');
    try {
        let changelog = await fs.promises.readFile(clPath, 'utf-8');
        const today = new Date().toISOString().slice(0, 10);
        changelog = changelog.replace(/## Unreleased/i, `## Unreleased\n\n## [${version}] - ${today}`);
        await fs.promises.writeFile(clPath, changelog, 'utf-8');
    }
    catch { /* CHANGELOG.md may not exist */ }
}
//# sourceMappingURL=release-wizard.js.map