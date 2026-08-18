//! #1 项目软删生产函数（审计收口）。
//!
//! 唯一 authority：Host 的 visible/hidden 集合决定侧栏投影；本函数是
//! 「删除项目」这一动作在生产调用链上的唯一实现——Renderer→Host
//! `project.remove` → 刷新 visible + hidden → 清理 active 投影 → 发布导航。
//! Daemon 会话/消息/run/磁盘不删除；重新添加（project.register）恢复。
//!
//! 抽成依赖注入的模块级函数是为了让红灯测试真实调用同一生产代码，
//! 而不是复制一份「helper 版」逻辑。

export interface RegisteredProjectRecord {
  id: string;
  path: string;
  lastOpenedAt?: string | null;
  label?: string;
  exists?: boolean;
}

export interface RemoveProjectDeps {
  api: {
    list(): Promise<Array<{ id: string; path: string }> | null>;
    listHidden(): Promise<string[] | null>;
    remove(id: string): Promise<void>;
  } | null;
  /** 目标路径（或其 id）。 */
  path: string;
  activeProjectPath: string | null;
  setRegisteredProjects: (projects: RegisteredProjectRecord[]) => void;
  setHiddenProjectPaths: (paths: string[]) => void;
  setActiveProjectPath: (path: string | null) => void;
  /** 只回传导航快照中与可见性相关的两个字段；调用方负责合并进完整快照。 */
  publishNavigation: (
    updater: (
      prev: { groups: Array<{ path: string | null }>; activeProjectPath: string | null },
    ) => { groups: Array<{ path: string | null }>; activeProjectPath: string | null },
  ) => void;
  writeActiveProject: (path: string | null) => Promise<void>;
  onError?: (message: string) => void;
}

/**
 * 删除（软删）一个项目并同步可见性投影。
 *
 * - Host `project.remove` 检查 affected rows；不存在/已删除会抛错 → 返回 false，
 *   不显示成功。
 * - 成功后刷新 visible + hidden 集合；hidden 读取失败时 fail-closed 返回 false
 *   （不能清空 hidden 让已删项目复活）。
 * - active 不停留在已隐藏项目：切到下一个可见项目或空态。
 * - 重新添加走 `project.register`（同一 Host 权威），本函数不触碰 Daemon 数据。
 */
export async function removeProjectFromHost(deps: RemoveProjectDeps): Promise<boolean> {
  const { api, path, activeProjectPath } = deps;
  try {
    if (!api) {
      deps.onError?.('Project API unavailable');
      return false;
    }
    const projects = (await api.list()) ?? [];
    const normalizedPath = normalizeAssistantProjectPath(path);
    const match = projects.find(
      (project) =>
        project.id === path ||
        normalizeAssistantProjectPath(project.path) === normalizedPath,
    );
    // Host 检查 affected rows；不存在或已删除会抛错，不假成功。
    await api.remove(match?.id ?? path);
    const next = (await api.list()) ?? [];
    const hidden = await api.listHidden();
    if (hidden === null) {
      // fail-closed：hidden 读取失败不能当作空集合，否则已删项目复活。
      deps.onError?.('Failed to refresh hidden projects after removal');
      return false;
    }
    if (next.some(
      (project) =>
        project.id === path ||
        normalizeAssistantProjectPath(project.path) === normalizedPath,
    )) {
      deps.onError?.('Project remains registered after removal');
      return false;
    }
    deps.setRegisteredProjects(next);
    deps.setHiddenProjectPaths(hidden);
    if (normalizeAssistantProjectPath(activeProjectPath ?? '') === normalizedPath) {
      // 不把 active 停留在已隐藏项目：选择下一个可见项目或空态。
      const nextPath = next.find(
        (project) => !hidden.some(
          (hiddenPath) =>
            normalizeAssistantProjectPath(hiddenPath) ===
            normalizeAssistantProjectPath(project.path),
        ),
      )?.path ?? null;
      deps.setActiveProjectPath(nextPath);
      void deps.writeActiveProject(nextPath);
    }
    deps.publishNavigation((prev) => ({
      ...prev,
      groups: prev.groups.filter((group) => {
        const groupPath = group.path ? normalizeAssistantProjectPath(group.path) : null;
        return groupPath !== normalizedPath && !hidden.some(
          (hiddenPath) => groupPath === normalizeAssistantProjectPath(hiddenPath),
        );
      }),
      activeProjectPath:
        normalizeAssistantProjectPath(activeProjectPath ?? '') === normalizedPath
          ? next.find(
              (project) => !hidden.some(
                (hiddenPath) =>
                  normalizeAssistantProjectPath(hiddenPath) ===
                  normalizeAssistantProjectPath(project.path),
              ),
            )?.path ?? null
          : prev.activeProjectPath,
    }));
    return true;
  } catch (err) {
    deps.onError?.(err instanceof Error ? err.message : String(err));
    return false;
  }
}
import { normalizeAssistantProjectPath } from './assistant-project-path';
