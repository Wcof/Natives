// ── File Types ──

/** 文件类型分类 */
export type FileKind = 'text' | 'image' | 'video' | 'audio' | 'pdf' | 'archive' | 'other';

/** 所有文件类型的列表 */
export const FILE_KINDS: FileKind[] = ['text', 'image', 'video', 'audio', 'pdf', 'archive', 'other'];

/** 项目徽章类型 */
export type ProjectBadge = 'node' | 'web' | 'python' | 'rust' | 'go' | 'git';

/** 所有项目徽章的列表 */
export const PROJECT_BADGES: ProjectBadge[] = ['node', 'web', 'python', 'rust', 'go', 'git'];

/** 文件条目 */
export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  kind: FileKind;
  hidden: boolean;
  size: number;
  mtime: number;
  btime: number;
  projectBadge?: ProjectBadge;
  symlink?: string;
  /** 文件所在目录（用于最近修改视图） */
  dirHint?: string;
}

/** 搜索结果 */
export interface SearchResult {
  path: string;
  name: string;
  score: number;
  isDir: boolean;
  mtime: number;
  matchRanges: [number, number][];
}

/** 内容搜索结果 */
export interface ContentSearchResult {
  path: string;
  name: string;
  line: number;
  preview: string;
  matchStart: number;
  matchEnd: number;
  score?: number;
  mtime?: number;
}

/** Git 状态 */
export interface GitStatus {
  root: string;
  branch: string;
  files: GitFileStatus[];
}

/** Git 文件状态 */
export interface GitFileStatus {
  path: string;
  status: 'M' | 'A' | 'D' | 'R' | '??' | 'UU';
  oldPath?: string;
}

/** 磁盘用量项 */
export interface DiskUsageItem {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  sizeFormatted: string;
}

/** 单模型统计（用于 usage.refresh() 响应） */
export interface ModelStat {
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens?: number;
  cacheReadTokens?: number;
  requestCount: number;
  totalTokens: number;
  totalCost: number;
  avgCostPerRequest: number;
}

/** 终端路径候选 */
export interface PathCandidate {
  path: string;
  exists: boolean;
}

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
export function detectFileKind(fileName: string): FileKind {
  // 检查无扩展名的文本文件
  if (EXTENSIONLESS_TEXT_FILES.has(fileName)) return 'text';

  const dotIndex = fileName.lastIndexOf('.');
  if (dotIndex === -1) return 'other';

  const ext = fileName.slice(dotIndex).toLowerCase();

  // 处理复合扩展名（如 .tar.gz）
  const secondDot = fileName.lastIndexOf('.', dotIndex - 1);
  if (secondDot !== -1) {
    const compoundExt = fileName.slice(secondDot).toLowerCase();
    if (ARCHIVE_EXTENSIONS.has(compoundExt)) return 'archive';
  }

  if (TEXT_EXTENSIONS.has(ext)) return 'text';
  if (IMAGE_EXTENSIONS.has(ext)) return 'image';
  if (VIDEO_EXTENSIONS.has(ext)) return 'video';
  if (AUDIO_EXTENSIONS.has(ext)) return 'audio';
  if (ext === '.pdf') return 'pdf';
  if (ARCHIVE_EXTENSIONS.has(ext)) return 'archive';

  return 'other';
}

/**
 * 根据目录中的文件检测项目徽章
 * @param dirPath 目录路径（仅用于 .git 检测）
 * @param fileNames 目录中的文件名列表
 * @param hasGitDir 是否有 .git 子目录
 */
export function detectProjectBadge(
  _dirPath: string,
  fileNames: string[],
  hasGitDir?: boolean,
): ProjectBadge | undefined {
  const nameSet = new Set(fileNames.map((f) => f.toLowerCase()));

  // 优先级最高：package.json → node
  if (nameSet.has('package.json')) return 'node';

  // index.html → web
  if (nameSet.has('index.html')) return 'web';

  // requirements.txt → python
  if (nameSet.has('requirements.txt') || nameSet.has('setup.py') || nameSet.has('pyproject.toml')) return 'python';

  // Cargo.toml → rust
  if (nameSet.has('cargo.toml')) return 'rust';

  // go.mod → go
  if (nameSet.has('go.mod')) return 'go';

  // .git 目录 → git（仅当没有其他项目文件时）
  if (hasGitDir) return 'git';

  return undefined;
}
