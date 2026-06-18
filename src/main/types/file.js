"use strict";
// ── File Types ──
Object.defineProperty(exports, "__esModule", { value: true });
exports.PROJECT_BADGES = exports.FILE_KINDS = void 0;
exports.detectFileKind = detectFileKind;
exports.detectProjectBadge = detectProjectBadge;
/** 所有文件类型的列表 */
exports.FILE_KINDS = ['text', 'image', 'video', 'audio', 'pdf', 'archive', 'other'];
/** 所有项目徽章的列表 */
exports.PROJECT_BADGES = ['node', 'web', 'python', 'rust', 'go', 'git'];
// ── 文本文件扩展名（40+） ──
const TEXT_EXTENSIONS = new Set([
    '.txt', '.md', '.markdown', '.mdown',
    '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs',
    '.json', '.jsonc', '.yaml', '.yml', '.toml', '.xml',
    '.css', '.scss', '.sass', '.less', '.styl',
    '.html', '.htm', '.xhtml',
    '.py', '.pyw', '.rb', '.php', '.java', '.kt', '.kts', '.scala',
    '.rs', '.go', '.mod', '.sum',
    '.c', '.h', '.cpp', '.hpp', '.cc', '.hh', '.cs',
    '.swift', '.m', '.mm',
    '.sh', '.bash', '.zsh', '.fish', '.ps1', '.bat',
    '.env', '.gitignore', '.dockerignore', '.editorconfig',
    '.vue', '.svelte', '.astro',
    '.sql', '.graphql', '.gql',
    '.conf', '.ini', '.cfg',
]);
const IMAGE_EXTENSIONS = new Set(['.jpg', '.jpeg', '.png', '.gif', '.webp', '.svg', '.ico', '.bmp', '.tiff', '.tif', '.avif']);
const VIDEO_EXTENSIONS = new Set(['.mp4', '.mov', '.avi', '.mkv', '.webm', '.m4v', '.wmv', '.flv']);
const AUDIO_EXTENSIONS = new Set(['.mp3', '.wav', '.flac', '.ogg', '.m4a', '.aac', '.wma', '.opus']);
const ARCHIVE_EXTENSIONS = new Set(['.zip', '.tar.gz', '.tar.bz2', '.tar.xz', '.tar', '.gz', '.bz2', '.xz', '.rar', '.7z', '.tgz', '.tbz2']);
// ── 无扩展名文本文件 ──
const EXTENSIONLESS_TEXT_FILES = new Set([
    'Dockerfile', 'Makefile', 'Gemfile', 'Rakefile',
    'CHANGELOG', 'README', 'LICENSE', 'VERSION',
    'Procfile', '.env', '.gitignore',
]);
/**
 * 根据文件名检测文件类型
 */
function detectFileKind(fileName) {
    // 检查无扩展名的文本文件
    if (EXTENSIONLESS_TEXT_FILES.has(fileName))
        return 'text';
    const dotIndex = fileName.lastIndexOf('.');
    if (dotIndex === -1)
        return 'other';
    const ext = fileName.slice(dotIndex).toLowerCase();
    // 处理复合扩展名（如 .tar.gz）
    const secondDot = fileName.lastIndexOf('.', dotIndex - 1);
    if (secondDot !== -1) {
        const compoundExt = fileName.slice(secondDot).toLowerCase();
        if (ARCHIVE_EXTENSIONS.has(compoundExt))
            return 'archive';
    }
    if (TEXT_EXTENSIONS.has(ext))
        return 'text';
    if (IMAGE_EXTENSIONS.has(ext))
        return 'image';
    if (VIDEO_EXTENSIONS.has(ext))
        return 'video';
    if (AUDIO_EXTENSIONS.has(ext))
        return 'audio';
    if (ext === '.pdf')
        return 'pdf';
    if (ARCHIVE_EXTENSIONS.has(ext))
        return 'archive';
    return 'other';
}
/**
 * 根据目录中的文件检测项目徽章
 * @param dirPath 目录路径（仅用于 .git 检测）
 * @param fileNames 目录中的文件名列表
 * @param hasGitDir 是否有 .git 子目录
 */
function detectProjectBadge(_dirPath, fileNames, hasGitDir) {
    const nameSet = new Set(fileNames.map((f) => f.toLowerCase()));
    // 优先级最高：package.json → node
    if (nameSet.has('package.json'))
        return 'node';
    // index.html → web
    if (nameSet.has('index.html'))
        return 'web';
    // requirements.txt → python
    if (nameSet.has('requirements.txt') || nameSet.has('setup.py') || nameSet.has('pyproject.toml'))
        return 'python';
    // Cargo.toml → rust
    if (nameSet.has('cargo.toml'))
        return 'rust';
    // go.mod → go
    if (nameSet.has('go.mod'))
        return 'go';
    // .git 目录 → git（仅当没有其他项目文件时）
    if (hasGitDir)
        return 'git';
    return undefined;
}
