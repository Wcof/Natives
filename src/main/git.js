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
exports.getGitStatus = getGitStatus;
exports.getGitDiff = getGitDiff;
const exec_file_1 = require("@/lib/exec-file");
const path = __importStar(require("path"));
/**
 * 获取 Git 状态（porcelain 格式解析）
 * @param dirPath 目录路径
 * @returns Git 状态信息，非 git 目录返回 null
 */
async function getGitStatus(dirPath) {
    try {
        const output = await (0, exec_file_1.execFilePromise)('git', ['status', '--porcelain', '-b'], { cwd: dirPath });
        const lines = output.split('\n').filter((l) => l.trim());
        // 解析分支行: ## main...origin/main
        const branchLine = lines.find((l) => l.startsWith('## '));
        if (!branchLine)
            return null;
        const branch = parseBranch(branchLine);
        // 解析文件状态
        const fileLines = lines.filter((l) => !l.startsWith('## '));
        const files = [];
        for (const line of fileLines) {
            const parsed = parseFileStatus(line);
            if (parsed)
                files.push(parsed);
        }
        return { root: dirPath, branch, files };
    }
    catch {
        return null; // 非 git 目录或 git 未安装
    }
}
function parseBranch(branchLine) {
    // ## main...origin/main [ahead 1]
    // ## HEAD (no branch)
    const rest = branchLine.slice(3).trim(); // 去掉 "## "
    const aheadIdx = rest.indexOf('...');
    if (aheadIdx !== -1)
        return rest.slice(0, aheadIdx);
    return rest;
}
function parseFileStatus(line) {
    // XY filename
    // R  old -> new
    // ?? filename
    const xy = line.slice(0, 2).trim();
    const rest = line.slice(3).trim();
    if (!xy)
        return null;
    // 映射 X/Y 状态到合并状态
    const status = mapStatus(xy);
    if (!status)
        return null;
    if (status === 'R' && rest.includes('->')) {
        const [oldPath, newPath] = rest.split('->').map((s) => s.trim());
        return { path: newPath || rest, status, oldPath };
    }
    return { path: rest, status };
}
function mapStatus(xy) {
    // 取右侧（working tree）状态，优先 staged 状态
    // 标准 porcelain: XY where X=staged, Y=working
    // 合并: UU = unmerged
    if (xy === '??')
        return '??';
    if (xy === 'UU')
        return 'UU';
    if (xy === 'R ')
        return 'R';
    if (xy === 'RM')
        return 'R';
    if (xy.includes('R'))
        return 'R';
    if (xy.includes('M'))
        return 'M';
    if (xy.includes('A'))
        return 'A';
    if (xy.includes('D'))
        return 'D';
    return null;
}
/**
 * 获取文件的 Git diff（HEAD vs working tree）
 * @param filePath 文件路径
 * @returns diff 文本，未跟踪或非 git 返回 null
 */
async function getGitDiff(filePath) {
    const dir = path.dirname(filePath);
    try {
        // 先检查是否在 git 仓库中
        await (0, exec_file_1.execFilePromise)('git', ['rev-parse', '--show-toplevel'], { cwd: dir });
    }
    catch {
        return null;
    }
    try {
        const diff = await (0, exec_file_1.execFilePromise)('git', ['diff', 'HEAD', '--', filePath], { cwd: dir });
        if (!diff.trim())
            return null;
        return diff;
    }
    catch {
        return null;
    }
}
//# sourceMappingURL=git.js.map