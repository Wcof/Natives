# Natives 全局国际化 (i18n) 完全对齐整改计划 (V3)

本计划旨在全面清理 `Natives` 项目中所有未被中英文设置控制的硬编码文字。通过在 `src/i18n` 中补全相应的词典，并在各前端 React 组件中绑定 `locale` 状态与 `t` 转换函数，确保对用户展示的内容 100% 实现中英文动态切换。

---

## 1. 🌐 扩展翻译词典字典 (`zh.ts` / `en.ts`)

我们需要在 `src/i18n/zh.ts` 和 `src/i18n/en.ts` 中补全并扩展以下键值对：

### 扩展 `common` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  common: {
    // ... 原有键
    close: '关闭',
    yes: '是',
    no: '否',
    unknown: '未知',
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  common: {
    // ... 原有键
    close: 'Close',
    yes: 'Yes',
    no: 'No',
    unknown: 'Unknown',
  }
  ```

### 扩展 `fileBrowser` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  fileBrowser: {
    // ... 原有键
    previewTab: '预览',
    codeTab: '代码',
    gitTab: '版本控制',
    infoTab: '信息',
    infoName: '名称',
    infoPath: '路径',
    infoType: '文件类型',
    infoSize: '大小',
    infoModified: '修改时间',
    infoCreated: '创建时间',
    infoHidden: '隐藏',
    infoDir: '文件夹',
    infoSymlink: '符号链接',
    infoProject: '项目',
    noPreview: '暂不支持预览 {kind} 类型文件。',
    failedLoad: '文件加载失败',
    loadingGit: '正在加载 Git 变更...',
    notInGit: '当前目录不处于 Git 仓库中',
    noGitChanges: '未检测到任何变更',
    showHidden: '隐藏文件',
    newFile: '新建文件',
    newFolder: '新建文件夹',
    searchPlaceholder: '搜索文件...',
    analyzing: '分析中...',
    confirmTrash: '确定要将 "{name}" 移入废纸篓吗？',
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  fileBrowser: {
    // ... 原有键
    previewTab: 'Preview',
    codeTab: 'Code',
    gitTab: 'Git',
    infoTab: 'Info',
    infoName: 'Name',
    infoPath: 'Path',
    infoType: 'Type',
    infoSize: 'Size',
    infoModified: 'Modified',
    infoCreated: 'Created',
    infoHidden: 'Hidden',
    infoDir: 'Directory',
    infoSymlink: 'Symlink',
    infoProject: 'Project',
    noPreview: 'No preview available for {kind} files.',
    failedLoad: 'Failed to load file',
    loadingGit: 'Loading git info...',
    notInGit: 'Not in a git repository',
    noGitChanges: 'No changes detected',
    showHidden: 'Hidden',
    newFile: 'New File',
    newFolder: 'New Folder',
    searchPlaceholder: 'Search files...',
    analyzing: 'Analyzing...',
    confirmTrash: 'Move "{name}" to trash?',
  }
  ```

### 扩展 `rightPanel` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  rightPanel: {
    // ... 原有键
    noNotifications: '暂无系统通知',
    selectFileToPreview: '选择一个文件进行预览',
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  rightPanel: {
    // ... 原有键
    noNotifications: 'No notifications',
    selectFileToPreview: 'Select a file to preview',
  }
  ```

### 扩展 `aiWorkbench` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  aiWorkbench: {
    // ... 原有键
    planFree: '免费版',
    organizer: {
      // ... 原有键
      actions: {
        move: '移动',
        rename: '重命名',
        delete: '删除',
        archive: '归档',
      }
    },
    skills: {
      confirmUninstall: '确定要卸载此技能吗？',
      enabled: '已启用',
      disabled: '已禁用',
      uninstall: '卸载',
      healthy: '正常',
      issues: '异常',
    }
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  aiWorkbench: {
    // ... 原有键
    planFree: 'Free',
    organizer: {
      // ... 原有键
      actions: {
        move: 'Move',
        rename: 'Rename',
        delete: 'Delete',
        archive: 'Archive',
      }
    },
    skills: {
      confirmUninstall: 'Are you sure you want to uninstall this skill?',
      enabled: 'Enabled',
      disabled: 'Disabled',
      uninstall: 'Uninstall',
      healthy: 'Healthy',
      issues: 'Issues',
    }
  }
  ```

### 扩展 `terminal` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  terminal: {
    // ... 原有键
    followModeOn: '跟随模式：开启 (终端跟随文件浏览器)',
    followModeOff: '跟随模式：关闭',
    restore: '还原终端',
    maximize: '最大化终端',
    open: '打开终端',
    close: '关闭终端',
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  terminal: {
    // ... 原有键
    followModeOn: 'Follow mode: ON (terminal follows file browser)',
    followModeOff: 'Follow mode: OFF',
    restore: 'Restore terminal',
    maximize: 'Maximize terminal',
    open: 'Open terminal',
    close: 'Close terminal',
  }
  ```

### 扩展 `commandPalette` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  commandPalette: {
    // ... 原有键
    navNavigate: '导航',
    navSelect: '选择',
    navClose: '关闭',
    navCycle: '循环',
    searchScopeGlobalTitle: '搜索范围：全盘',
    searchScopeLocalTitle: '搜索范围：用户目录',
    searchScopeGlobal: '🌐 全局',
    searchScopeLocal: '📁 本地',
    static: {
      openSettings: '打开设置',
      openWorkshop: '打开工坊',
      openStore: '打开商店',
      openNotifications: '打开通知',
      fileBrowser: '文件浏览器',
      aiWorkbench: 'AI 工作台',
      tools: '工具',
      toggleTerminal: '切换终端',
      themeVolt: '主题: Terminal Volt',
      themeWarm: '主题: Warm Archive',
      themeEditorial: '主题: Editorial',
    }
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  commandPalette: {
    // ... 原有键
    navNavigate: 'Navigate',
    navSelect: 'Select',
    navClose: 'Close',
    navCycle: 'Cycle',
    searchScopeGlobalTitle: 'Searching: Full disk',
    searchScopeLocalTitle: 'Searching: Home directory',
    searchScopeGlobal: '🌐 Global',
    searchScopeLocal: '📁 Local',
    static: {
      openSettings: 'Open Settings',
      openWorkshop: 'Open Workshop',
      openStore: 'Open Store',
      openNotifications: 'Open Notifications',
      fileBrowser: 'File Browser',
      aiWorkbench: 'AI Workbench',
      tools: 'Tools',
      toggleTerminal: 'Toggle Terminal',
      themeVolt: 'Theme: Terminal Volt',
      themeWarm: 'Theme: Warm Archive',
      themeEditorial: 'Theme: Editorial',
    }
  }
  ```

### 扩展 `screenshot` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  screenshot: {
    // ... 原有键
    enterTextPlaceholder: '输入文本...',
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  screenshot: {
    // ... 原有键
    enterTextPlaceholder: 'Enter text...',
  }
  ```

### 扩展 `release` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  release: {
    // ... 原有键
    projectPathPlaceholder: '请输入项目路径',
    log: {
      error: '错误',
      inspected: '已完成项目审计',
      inspectFailed: '审计失败',
      prepared: '已就绪发布版本',
      prepareFailed: '就绪处理失败',
      completed: '已完成',
      failed: '失败',
    }
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  release: {
    // ... 原有键
    projectPathPlaceholder: 'e.g. /path/to/your/project',
    log: {
      error: 'Error',
      inspected: 'Inspected project',
      inspectFailed: 'Inspection failed',
      prepared: 'Prepared release',
      prepareFailed: 'Prepare failed',
      completed: 'completed',
      failed: 'failed',
    }
  }
  ```

### 扩展 `settings` 空间
* **中文 (`zh.ts`)**:
  ```typescript
  settings: {
    // ... 原有键
    themes: {
      voltDesc: '暗黑风格，航天终端感',
      warmDesc: '温馨纸张质感，护眼',
      editorialDesc: '极简 brutalist 风格，高对比度',
    }
  }
  ```
* **英文 (`en.ts`)**:
  ```typescript
  settings: {
    // ... 原有键
    themes: {
      voltDesc: 'Dark, terminal-inspired',
      warmDesc: 'Warm, paper-like',
      editorialDesc: 'Brutalist, high contrast',
    }
  }
  ```

---

## 2. 🛠️ 具体组件重构修改规范

对以下被审计组件中的硬编码字符进行规范重构：

### ① [FilePreview.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/files/FilePreview.tsx)
* **重构指令**:
  1. 引入 `Locale` 和 `t`；
  2. 使用 `useState<Locale>('zh')` 声明状态，并在 `useEffect` 中挂载 `window.nativesAPI.getLocale().then(...)`；
  3. 将 `tabs` 数组中的 `label` 重构为：
     - `'Preview'` -> `t(locale, 'fileBrowser.previewTab')`
     - `'Code'` -> `t(locale, 'fileBrowser.codeTab')`
     - `'Git'` -> `t(locale, 'fileBrowser.gitTab')`
     - `'Info'` -> `t(locale, 'fileBrowser.infoTab')`
  4. 将 Git 动作及状态映射中的文本提取为 i18n 字典词：
     - `'Modified'` -> `t(locale, 'common.modified')`
     - `'Added'` -> `t(locale, 'common.added')`
     - `'Deleted'` -> `t(locale, 'common.deleted')`
     - `'Renamed'` -> `t(locale, 'common.renamed')`
     - `'Untracked'` -> `t(locale, 'common.untracked')`
     - `'Conflict'` -> `t(locale, 'common.conflicted')`
     - `'Unchanged'` -> `t(locale, 'common.unchanged')`
     - `'Not in a git repository'` -> `t(locale, 'fileBrowser.notInGit')`
     - `'No changes detected'` -> `t(locale, 'fileBrowser.noGitChanges')`
     - `'Loading git info...'` -> `t(locale, 'fileBrowser.loadingGit')`
  5. 替换 `FileInfo` 表格的 Key，由 `'Name'`, `'Path'` 等硬编码替换为 `t(locale, 'fileBrowser.infoName')`, `t(locale, 'fileBrowser.infoPath')`, `t(locale, 'fileBrowser.infoType')`, `t(locale, 'fileBrowser.infoSize')`, `t(locale, 'fileBrowser.infoModified')`, `t(locale, 'fileBrowser.infoCreated')`, `t(locale, 'fileBrowser.infoHidden')`，以及布尔值或行项目的值 `'Yes'` / `'No'` -> `t(locale, 'common.yes')` / `t(locale, 'common.no')`；
  6. 兜底无预览提示替换为：`t(locale, 'fileBrowser.noPreview').replace('{kind}', entry.kind)`。

### ② [FileToolbar.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/files/FileToolbar.tsx)
* **重构指令**:
  1. 升降序 hover 提示 `title` 替换为：`sortDir === 'asc' ? t(locale, 'fileBrowser.ascending') : t(locale, 'fileBrowser.descending')`；
  2. 隐藏文件文本 `'Hidden'` 替换为：`t(locale, 'fileBrowser.showHidden')`；
  3. 新建按钮与 hover 提示替换为：
     - `'New File'` / `'+ File'` -> `t(locale, 'fileBrowser.newFile')`
     - `'New Folder'` / `'+ Folder'` -> `t(locale, 'fileBrowser.newFolder')`
  4. 搜索框 `'Search files...'` 占位符替换为：`t(locale, 'fileBrowser.searchPlaceholder')`。

### ③ [RightPanel.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/shell/RightPanel.tsx)
* **重构指令**:
  - 第129行空状态文本 `'No notifications'` 与 `'Select a file to preview'` 分别替换为：
    - `t(locale, 'rightPanel.noNotifications')`
    - `t(locale, 'rightPanel.selectFileToPreview')`

### ④ [DiskUsage.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/files/DiskUsage.tsx)
* **重构指令**:
  - 第44行中的 `'Analyzing...'` 替换为：`t(locale, 'fileBrowser.analyzing')`。

### ⑤ [FileBrowser.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/files/FileBrowser.tsx)
* **重构指令**:
  1. 第161行 `confirm` 弹窗 `'Move "{entry.name}" to trash?'` 改为：`confirm(t(locale, 'fileBrowser.confirmTrash').replace('{name}', entry.name))`；
  2. 弹窗和按钮中的硬编码替换为：
    - `'Rename'` -> `t(locale, 'fileBrowser.rename')`
    - `'Cancel'` -> `t(locale, 'common.cancel')`
    - `'Create'` -> `t(locale, 'common.confirm')`
    - `'New File'` / `'New Folder'` -> `t(locale, 'fileBrowser.newFile')` / `t(locale, 'fileBrowser.newFolder')`
    - `'Loading...'` -> `t(locale, 'common.loading')`

### ⑥ [AIFileOrganizer.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/ai/AIFileOrganizer.tsx)
* **重构指令**:
  - 第206行中的 `'📦 Move'`, `'✏️ Rename'`, `'🗑️ Delete'`, `'📁 Archive'` 动作按钮文字映射到国际化配置：
    - `'📦 Move'` -> `t(locale, 'aiWorkbench.organizer.actions.move')`
    - `'✏️ Rename'` -> `t(locale, 'aiWorkbench.organizer.actions.rename')`
    - `'🗑️ Delete'` -> `t(locale, 'aiWorkbench.organizer.actions.delete')`
    - `'📁 Archive'` -> `t(locale, 'aiWorkbench.organizer.actions.archive')`

### ⑦ [SkillsPanel.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/ai/SkillsPanel.tsx)
* **重构指令**:
  1. 第55行确认弹框的 `'确定要卸载此技能吗？'` 替换为：`confirm(t(locale, 'aiWorkbench.skills.confirmUninstall'))`；
  2. `'已启用'` / `'已禁用'` 替换为：`skill.enabled ? t(locale, 'aiWorkbench.skills.enabled') : t(locale, 'aiWorkbench.skills.disabled')`；
  3. `'卸载'` 按钮文本替换为：`t(locale, 'aiWorkbench.skills.uninstall')`；
  4. 筛选标签的 `f` 变量（即 `'healthy'` / `'issues'`) 渲染文本，应使用：`f === 'healthy' ? t(locale, 'aiWorkbench.skills.healthy') : f === 'issues' ? t(locale, 'aiWorkbench.skills.issues') : f`。

### ⑧ [Terminal.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/shell/Terminal.tsx)
* **重构指令**:
  1. 第403行跟随模式的 `title` 提示改写为：`followMode ? t(locale, 'terminal.followModeOn') : t(locale, 'terminal.followModeOff')`；
  2. 第409和418行 `aria-label` 中的无障碍英文标签改写为：
     - `'Restore terminal'` / `'Maximize terminal'` -> `isMaximized ? t(locale, 'terminal.restore') : t(locale, 'terminal.maximize')`
     - `'Open terminal'` / `'Close terminal'` -> `isCollapsed ? t(locale, 'terminal.expand') : t(locale, 'terminal.collapse')`

### ⑨ [CommandPalette.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/shell/CommandPalette.tsx)
* **重构指令**:
  1. `STATIC_COMMANDS` 数组的 `label` 部分，不能直接在静态变量中写死，应该在组件内部通过 `locale` 动态生成。将 `STATIC_COMMANDS` 重构为函数或者在组件内进行 `results.map` 时根据 `cmd.id` 转换其渲染的 label 文本，比如：
     - `'Open Settings'` -> `t(locale, 'commandPalette.static.openSettings')`
     - `'Open Workshop'` -> `t(locale, 'commandPalette.static.openWorkshop')`
     - ... 依此类推。
  2. 底部热键说明 `'Navigate'`, `'Select'`, `'Close'`, `'Cycle'` 替换为：`t(locale, 'commandPalette.navNavigate')`, `t(locale, 'commandPalette.navSelect')`, `t(locale, 'commandPalette.navClose')`, `t(locale, 'commandPalette.navCycle')`；
  3. 全局/本地搜索范围切换按钮文本 `Global` / `Local` 及其 hover 提示：
     - `'Searching: Full disk'` -> `t(locale, 'commandPalette.searchScopeGlobalTitle')`
     - `'Searching: Home directory'` -> `t(locale, 'commandPalette.searchScopeLocalTitle')`
     - `'🌐 Global'` -> `t(locale, 'commandPalette.searchScopeGlobal')`
     - `'📁 Local'` -> `t(locale, 'commandPalette.searchScopeLocal')`

### ⑩ [AnnotationEditor.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/screenshot/AnnotationEditor.tsx)
* **重构指令**:
  - 批注文本弹窗中的 `placeholder="Enter text..."` 替换为：`t(locale, 'screenshot.enterTextPlaceholder')`。

### ⑪ [ReleaseWizardDialog.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/release/ReleaseWizardDialog.tsx)
* **重构指令**:
  1. 路径输入框的占位符 `'placeholder="/path/to/your/project"'` 替换为：`t(locale, 'release.projectPathPlaceholder')`；
  2. 对于 `setLog` 日志记录的状态拼接进行多语言化：
     - `'Error: '` -> `${t(locale, 'common.error')}: `
     - `'Inspected '` -> `${t(locale, 'release.log.inspected')} `
     - `'Inspection failed: '` -> `${t(locale, 'release.log.inspectFailed')}: `
     - `'Prepared release v'` -> `${t(locale, 'release.log.prepared')} v`
     - `'Prepare failed: '` -> `${t(locale, 'release.log.prepareFailed')}: `
     - `' completed'` -> ` ${t(locale, 'release.log.completed')}`
     - `' failed'` -> ` ${t(locale, 'release.log.failed')}`

### ⑫ [SettingsPage.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/shell/SettingsPage.tsx)
* **重构指令**:
  - `THEMES` 数组中各主题的描述 `desc` 映射为 i18n 键值：
    - `'Dark, terminal-inspired'` -> `t(locale, 'settings.themes.voltDesc')`
    - `'Warm, paper-like'` -> `t(locale, 'settings.themes.warmDesc')`
    - `'Brutalist, high contrast'` -> `t(locale, 'settings.themes.editorialDesc')`

### ⑬ [UsagePanel.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/ai/UsagePanel.tsx)
* **重构指令**:
  - 第109行中 `'Free'` 替换为：`t(locale, 'aiWorkbench.planFree')`。

---

## 3. 🧪 验证与自测计划

1. **编译检查**:
   在终端执行以下命令，确保没有任何类型报错：
   ```bash
   npx tsc --noEmit
   ```
2. **多语言切换状态验证**:
   - 在应用设置中切换语言为 **中文**，观察文件浏览器（重命名、新建、上下文菜单）、右侧预览面板（文件属性、Git Diff状态、无法预览提示）、AI 工作台（文件整理、技能管理、使用率看板）、终端面板、命令面板（热键提示、静态命令列表、搜索切换），确保无英文残留。
   - 切换为 **英文**，检查上述相应区域，确保英文展现完全，翻译词条占位符正确替换。

---

## 4. 📝 漏网之鱼补充整改计划 (V3 追加)

经过深度代码扫描，我们发现了以下最后 3 处可能在切换多语言时产生死角的硬编码文字，请进行二次整改：

### ① [SkillsPanel.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/ai/SkillsPanel.tsx)
* **硬编码现状**: 
  - L55 `confirm('确定要卸载此技能吗？')` 写死中文。
  - L130 `{skill.enabled ? '已启用' : '已禁用'}` 写死中文。
  - L154 `卸载` 按钮写死中文。
  - L89 `f === 'all' ? t(locale, 'aiWorkbench.skills') : f` 中的 `f`（即 `'healthy'` / `'issues'`) 渲染为硬编码英文。
* **整改方案**:
  1. 引入 `t` 和 `locale` 状态绑定。
  2. 卸载确认：`confirm(t(locale, 'aiWorkbench.skills.confirmUninstall'))`
  3. 状态标签：`skill.enabled ? t(locale, 'aiWorkbench.skills.enabled') : t(locale, 'aiWorkbench.skills.disabled')`
  4. 卸载按钮：`t(locale, 'aiWorkbench.skills.uninstall')`
  5. 过滤器标签：`f === 'healthy' ? t(locale, 'aiWorkbench.skills.healthy') : f === 'issues' ? t(locale, 'aiWorkbench.skills.issues') : f`

### ② [WorkshopPage.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/components/shell/WorkshopPage.tsx)
* **硬编码现状**:
  - L535 `PERMISSION_DESC` 权限描述字典为中文硬编码。
* **整改方案**:
  1. 将字典翻译迁移至 i18n 配置文件。
  2. 新增 `workshop.permissions` 字典键：
     - `'db:read'`, `'db:write'`, `'env:read'`, `'notification'`, `'ipc:send'`, `'lifecycle'`, `'settings'`
  3. 在渲染时动态获取：
     - `PERMISSION_DESC[perm]` 替换为 `t(locale, 'workshop.permissions.' + perm)`

### ③ [layout.tsx](file:///Users/ldh/Downloads/project/AiNative/Natives/src/app/layout.tsx)
* **硬编码现状**:
  - L7 `description: 'AI Steam Base — 桌面应用容器'` 描述文本为中文硬编码。
  - L16 `<html lang="zh-CN">` 语言表示为硬编码。
* **整改方案**:
  1. 移除 `lang="zh-CN"` 以避免初始化冲突，改由客户端 `ShellLayout` 在初始化时写入正确的 `lang`；或根据本地存储在渲染时同步（如果为 Next.js 静态/SSR 可选做）。
  2. 建议将 `description` 更改为对应语言（因桌面基座无需大量 SEO，也可保留或以最精简的英文作为默认描述）。

