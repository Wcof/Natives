/**
 * linked-worktree（UX-13/14 · W8）— 只读 worktree 解析 adapter。
 *
 * Host 侧 `git.rs::GitBranch.worktree_path` 通过 `git worktree list --porcelain`
 * 只读解析：主 worktree 无 branch 行，linked worktree 有 `branch refs/heads/...`。
 * Renderer 无法运行 git（不修改 src-tauri，不新增 command），因此本 adapter
 * 用等价只读判据复现同一语义：
 *
 *   一个目录是 linked worktree ⟺ 其 `.git` 是一个普通文本文件，内容以
 *   `gitdir:` 开头（指向 <main>/.git/worktrees/<name>）。主 worktree 的 `.git`
 *   是目录（读取会失败）。该判据与 `git worktree list --porcelain` 的
 *   worktree/branch 行一一对应，且全程只读 —— 不创建/删除/prune/repair/
 *   切换分支。
 *
 * 数据源只复用现有只读 adapter：
 *   - window.nativesAPI.git.status(projectPath)  → 当前分支名
 *   - window.nativesAPI.fs.readFile(projectPath/.git) → linked worktree 判定
 */

export type LinkedWorktreeState =
  /** 会话未绑定项目路径（不显示 chip）。 */
  | 'none'
  /** Tauri IPC 不可用（浏览器 dev 无 git/fs 能力）。 */
  | 'unavailable'
  /** 项目存在但既不是 git 仓库也读不出 .git（非 git 目录）。 */
  | 'notGit'
  /** 项目是 git 主 worktree（无 linked worktree 绑定）。 */
  | 'main'
  /** 项目是 linked worktree（chip 显示该路径）。 */
  | 'linked'
  /** 探测过程出错（诚实 error 态）。 */
  | 'error';

export interface LinkedWorktreeResolution {
  state: LinkedWorktreeState;
  /** 会话绑定的 cwd/project_path。 */
  projectPath: string | null;
  /** 当前分支名（来自 git.status，可为 null）。 */
  branch: string | null;
  /** linked worktree 时 = projectPath 本身；否则 null。 */
  worktreePath: string | null;
  /** `.git` 文件 `gitdir:` 指向的管理目录（仅 linked 时有值）。 */
  gitDir: string | null;
  error: string | null;
}

/** 纯函数：解析 `.git` 文件内容 → 是否 linked worktree。可单测。 */
export function parseGitDirFile(content: string): {
  linked: boolean;
  gitDir: string | null;
} {
  const firstLine = content.split(/\r?\n/, 1)[0] ?? '';
  const trimmed = firstLine.trim();
  if (trimmed.startsWith('gitdir:')) {
    const gitDir = trimmed.slice('gitdir:'.length).trim();
    return { linked: true, gitDir: gitDir.length > 0 ? gitDir : null };
  }
  return { linked: false, gitDir: null };
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * 只读解析会话绑定的 projectPath 是否为 linked worktree。
 * 永不写盘、永不执行 git 变更命令。
 */
export async function resolveLinkedWorktree(
  projectPath: string | null | undefined,
): Promise<LinkedWorktreeResolution> {
  if (!projectPath) {
    return { state: 'none', projectPath: null, branch: null, worktreePath: null, gitDir: null, error: null };
  }
  const api = window.nativesAPI;
  if (!api?.fs?.readFile || !api?.git?.status) {
    return { state: 'unavailable', projectPath, branch: null, worktreePath: null, gitDir: null, error: null };
  }

  let branch: string | null = null;
  try {
    const status = (await api.git.status(projectPath)) as unknown;
    if (isPlainObject(status) && typeof status.branch === 'string') {
      branch = status.branch;
    }
  } catch {
    // git.status 失败 → 可能不是 git 仓库；交给 .git 判定兜底
  }

  try {
    const raw = await api.fs.readFile(`${projectPath}/.git`);
    const content =
      typeof raw === 'string'
        ? raw
        : isPlainObject(raw) && typeof raw.content === 'string'
          ? raw.content
          : '';
    const parsed = parseGitDirFile(content);
    if (parsed.linked) {
      return {
        state: 'linked',
        projectPath,
        branch,
        worktreePath: projectPath,
        gitDir: parsed.gitDir,
        error: null,
      };
    }
    // .git 可读但不是 gitdir 文件 → 异常内容，诚实 error 态
    return { state: 'error', projectPath, branch, worktreePath: null, gitDir: null, error: '.git is not a gitdir file' };
  } catch {
    // .git 读取失败（通常是目录 = 主 worktree）→ 非 linked
    if (branch) {
      return { state: 'main', projectPath, branch, worktreePath: null, gitDir: null, error: null };
    }
    return { state: 'notGit', projectPath, branch: null, worktreePath: null, gitDir: null, error: null };
  }
}
