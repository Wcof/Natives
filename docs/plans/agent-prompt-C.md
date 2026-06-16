# Agent C 提示词 — Sidebar 重构 + 主题引擎升级 + Layout 集成

```
你是一个前端集成工程师。你的任务是为一个 Electron + Next.js 15 + React 19 项目完成 Sidebar 组件重构、主题引擎升级和 Layout 集成。

## 项目信息

- 项目路径: /Users/ldh/Downloads/project/AiNative/Natives
- 技术栈: Electron 34 + Next.js 15 (App Router) + React 19 + TypeScript
- 路径别名: @/* 映射到 ./src/*
- 包管理器: pnpm

## 前置依赖（其他 agent 已完成）

1. Tailwind CSS 已安装并配置好（tailwind.config.ts, postcss.config.mjs, globals.css 已更新）
2. 以下 3 个新文件已创建：
   - src/context/ThemeContext.tsx — 导出 ThemeProvider, useTheme, useCanvasQuota
   - src/components/ui/LiquidGlass.tsx — 导出 LiquidGlass 组件（isActive, children, className, style）
   - src/components/ui/Modal.tsx — 导出 Modal 组件（isOpen, onClose, title, children, width, showCloseButton）

你需要先读取这些文件了解它们的接口，然后完成下面的任务。

## 你的任务

### 1. 升级 src/lib/theme-engine.ts

先读取当前文件，然后做以下改动：
- 保留现有的 ThemeSchema（Zod schema 不变）
- 删除 THEMES 中的 'editorial' 和 'warm-archive'
- 新增 'liquid-glass'（深色）和 'liquid-glass-light'（浅色）
- 更新 TERMINAL_THEMES 映射
- getThemeId() 默认值改为 'liquid-glass'

新主题定义：

liquid-glass (深色):
  bg: '#0b0c0a', bg-2: '#131410', bg-3: '#1c1e17',
  panel: '#1E211A', border: '#262920',
  text: '#f2f2ea', text-dim: '#9b9d8c', text-faint: '#62655a',
  accent: '#cdf24b', accent-ink: '#0b0c0a',
  radius: '4px', font-display: 'var(--font-ui)',
  danger: '#ef4444', danger-soft: '#ef444415',
  warning: '#eab308', info: '#3b82f6',
  diff-add: '#22c55e', diff-del: '#ef4444', diff-mod: '#3b82f6'

liquid-glass-light (浅色):
  bg: '#f4f1ea', bg-2: '#ece8dd', bg-3: '#e4dfd2',
  panel: '#f4f1ea', border: '#d6d1c3',
  text: '#0a0a0a', text-dim: '#57534a', text-faint: '#8a857a',
  accent: '#262920', accent-ink: '#ffffff',
  radius: '4px', font-display: 'var(--font-ui)',
  danger: '#dc2626', danger-soft: '#dc262615',
  warning: '#d97706', info: '#2563eb',
  diff-add: '#16a34a', diff-del: '#dc2626', diff-mod: '#2563eb'

TERMINAL_THEMES:
  liquid-glass: { background: '#0b0c0a', foreground: '#f2f2ea', cursor: '#cdf24b', selectionBackground: '#cdf24b33' }
  liquid-glass-light: { background: '#f4f1ea', foreground: '#0a0a0a', cursor: '#262920', selectionBackground: '#26292033' }

### 2. 重写 src/app/layout.tsx

先读取当前文件，然后重写为：

```tsx
import type { Metadata } from 'next';
import './globals.css';
import ShellLayout from '@/components/shell/ShellLayout';
import { ToastProvider } from '@/components/ui/Toast';
import { ThemeProvider } from '@/context/ThemeContext';

export const metadata: Metadata = {
  title: 'Natives',
  description: 'Natives — AI Steam Base',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html data-theme="liquid-glass" className="dark">
      <body>
        <ThemeProvider defaultTheme="liquid-glass">
          <ToastProvider>
            <ShellLayout>{children}</ShellLayout>
          </ToastProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
```

### 3. 重构 src/components/shell/Sidebar.tsx

先读取当前文件，然后做以下改动：

改动原则：
- 保留所有业务逻辑不变（拖拽排序、收藏、模块加载、i18n 等）
- 将 CSS 类名从 globals.css 自定义类迁移到 Tailwind utility classes
- active 的导航项用 <LiquidGlass isActive={true}> 包裹
- 非 active 项用 Tailwind utility classes
- active 项文字用 text-brand-jade-glow font-semibold

迁移对照：
  .sidebar → flex flex-col h-full bg-surface-panel border-r border-surface-border overflow-y-auto min-w-[190px] max-w-[420px] transition-all duration-300 ease-out-expo
  .sidebar.collapsed → w-0 !min-w-0 overflow-hidden p-0 border-none
  .sidebar-header → flex items-center justify-between px-3 pt-3 pb-2
  .sidebar-brand → text-[17px] font-bold tracking-[1px] text-accent
  .sidebar-section-title → text-[11px] uppercase tracking-[1px] text-content-text-faint px-3 pt-2 pb-1 mb-1
  .sidebar-item (普通) → flex items-center gap-2 py-[7px] px-2 mx-1 rounded-component cursor-pointer transition-colors duration-150 text-[13px] text-content-text-dim hover:bg-surface-bg-3 hover:text-content-text
  .sidebar-item (active) → 用 LiquidGlass 包裹，内部 text-brand-jade-glow font-semibold
  .sidebar-footer → mt-auto px-3 pb-3 pt-2 border-t border-surface-border
  .sidebar-drag-handle → absolute right-[-3px] top-0 bottom-0 w-[6px] cursor-col-resize opacity-0 hover:opacity-100 transition-opacity

引入 LiquidGlass：
  import LiquidGlass from '@/components/ui/LiquidGlass';

模块列表渲染示例：
  {modules.map((mod, index) => {
    const mId = getModuleId(mod);
    const mName = getModuleName(mod);
    const mIcon = getModuleIcon(mod);
    const isActive = activeModuleId === mId;

    return (
      <div
        key={mId}
        role="option"
        aria-selected={isActive}
        tabIndex={0}
        draggable
        onDragStart={() => handleDragStart(index)}
        onDragOver={(e) => handleDragOver(e, index)}
        onClick={() => onModuleSelect(mId)}
        onKeyDown={(e) => { if (e.key === 'Enter') onModuleSelect(mId); }}
        title={mName}
      >
        <LiquidGlass
          isActive={isActive}
          className="flex items-center gap-2 py-[7px] px-2 mx-1 rounded-component cursor-pointer transition-colors duration-150 text-[13px]"
          style={isActive ? {} : { color: 'var(--text-dim)' }}
        >
          <span>
            {mIcon ? (
              <img src={mIcon} alt="" style={{ width: 18, height: 18 }} />
            ) : (
              <Square size={18} />
            )}
          </span>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>{mName}</span>
        </LiquidGlass>
      </div>
    );
  })}

Quick Access 和 Favorites 中的非 active 项不需要 LiquidGlass，直接用 Tailwind 类。

Footer 的按钮（Notifications, Settings, Workshop）也不需要 LiquidGlass。

### 4. 最小改动 src/components/shell/ShellLayout.tsx

先读取当前文件，找到 applyTheme 的调用处，将默认主题从 'editorial' 改为 'liquid-glass'。
如果有 data-theme="editorial" 的硬编码引用，也改为 'liquid-glass'。
不要改动 ShellLayout 的其他逻辑。

## 绝对不要修改的文件

- tailwind.config.ts（另一个 agent 创建的）
- postcss.config.mjs（另一个 agent 创建的）
- src/context/ThemeContext.tsx（另一个 agent 创建的）
- src/components/ui/LiquidGlass.tsx（另一个 agent 创建的）
- src/components/ui/Modal.tsx（另一个 agent 创建的）
- src/lib/design-tokens.ts
- src/app/globals.css（另一个 agent 修改的）
- 其他组件文件（Header, ContentArea, Terminal 等不在本次范围内）
- electron/ 下任何文件

## 验收标准

1. pnpm dev 启动无报错
2. Sidebar 的 active 项有液态玻璃视觉效果（WebGL 或 CSS 降级）
3. Sidebar 的非 active 项使用 Tailwind utility classes 渲染
4. theme-engine.ts 中只有 liquid-glass 和 liquid-glass-light 两个主题
5. layout.tsx 使用 ThemeProvider 包裹
6. 默认主题为 liquid-glass（深色）
7. TypeScript 无类型错误（运行 tsc --noEmit 验证）

## 合并冲突预防

你修改的文件与其他 agent 完全不重叠：
- Sidebar.tsx — 你独占
- layout.tsx — 你独占
- theme-engine.ts — 你独占
- ShellLayout.tsx — 你只改默认主题字符串
```
