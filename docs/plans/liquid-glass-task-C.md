# Task C: Sidebar 重构 + 主题引擎升级 + Layout 集成

## 目标

将 Sidebar 从纯 CSS 类迁移到 Tailwind utility classes + LiquidGlass 组件，升级 theme-engine.ts 支持新的双主题（liquid-glass 深色 + liquid-glass-light 浅色），在 layout.tsx 中注入 ThemeProvider。

## 依赖

- **依赖 Task A 的产出**：`tailwind.config.ts` 和更新后的 `globals.css`（Tailwind utility classes 可用）
- **依赖 Task B 的产出**：`ThemeContext.tsx`、`LiquidGlass.tsx`、`Modal.tsx`

**注意**：本任务需要 Task A 和 Task B 都完成后才能进行。如果需要并行开发，可以先用 mock 文件搭建框架，等其他任务交付后再替换。

## 交付物

### 1. 升级 `src/lib/theme-engine.ts`

**改动要点**：
- 保留现有的 `ThemeSchema`（Zod schema 不变）
- 删除 `editorial` 和 `warm-archive` 两个旧主题
- 新增 `liquid-glass`（深色）和 `liquid-glass-light`（浅色）两个主题
- 更新 `TERMINAL_THEMES` 映射
- `applyTheme()` 函数逻辑不变
- `getThemeId()` 默认值改为 `'liquid-glass'`

**新主题定义**：

```ts
export const THEMES: Record<string, Theme> = {
  'liquid-glass': {
    bg: '#0b0c0a',
    'bg-2': '#131410',
    'bg-3': '#1c1e17',
    panel: '#1E211A',
    border: '#262920',
    text: '#f2f2ea',
    'text-dim': '#9b9d8c',
    'text-faint': '#62655a',
    accent: '#cdf24b',
    'accent-ink': '#0b0c0a',
    radius: '4px',
    'font-display': 'var(--font-ui)',
    danger: '#ef4444',
    'danger-soft': '#ef444415',
    warning: '#eab308',
    info: '#3b82f6',
    'diff-add': '#22c55e',
    'diff-del': '#ef4444',
    'diff-mod': '#3b82f6',
  },
  'liquid-glass-light': {
    bg: '#f4f1ea',
    'bg-2': '#ece8dd',
    'bg-3': '#e4dfd2',
    panel: '#f4f1ea',
    border: '#d6d1c3',
    text: '#0a0a0a',
    'text-dim': '#57534a',
    'text-faint': '#8a857a',
    accent: '#262920',
    'accent-ink': '#ffffff',
    radius: '4px',
    'font-display': 'var(--font-ui)',
    danger: '#dc2626',
    'danger-soft': '#dc262615',
    warning: '#d97706',
    info: '#2563eb',
    'diff-add': '#16a34a',
    'diff-del': '#dc2626',
    'diff-mod': '#2563eb',
  },
};

export const TERMINAL_THEMES: Record<string, { background: string; foreground: string; cursor: string; selectionBackground?: string }> = {
  'liquid-glass': {
    background: '#0b0c0a',
    foreground: '#f2f2ea',
    cursor: '#cdf24b',
    selectionBackground: '#cdf24b33',
  },
  'liquid-glass-light': {
    background: '#f4f1ea',
    foreground: '#0a0a0a',
    cursor: '#262920',
    selectionBackground: '#26292033',
  },
};
```

### 2. 重写 `src/app/layout.tsx`

**改动要点**：
- 引入 `ThemeProvider` 包裹整个应用
- 默认主题改为 `liquid-glass`
- `data-theme` 改为 `liquid-glass`
- 保留 `ToastProvider`

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

### 3. 重构 `src/components/shell/Sidebar.tsx`

**改动要点**：
- 将所有 CSS 类名从 `globals.css` 的自定义类迁移到 Tailwind utility classes
- 活跃（active）的导航项使用 `<LiquidGlass isActive={true}>` 包裹
- 非活跃项使用 Tailwind utility classes（如 `hover:bg-white/[0.03]`）
- 活跃项文字使用 `text-brand-jade-glow font-semibold`
- 保留所有业务逻辑（拖拽排序、收藏、模块加载等）不变

**迁移对照表**：

| 原 CSS 类 | Tailwind 替代 |
|-----------|---------------|
| `.sidebar` | `flex flex-col h-full bg-surface-panel border-r border-surface-border overflow-y-auto min-w-[var(--sidebar-min)] max-w-[var(--sidebar-max)] transition-all duration-slow ease-out-expo` |
| `.sidebar.collapsed` | `w-0 !min-w-0 overflow-hidden p-0 border-none` |
| `.sidebar-header` | `flex items-center justify-between px-3 pt-3 pb-2` |
| `.sidebar-brand` | `text-[17px] font-bold tracking-[1px] text-accent font-display` |
| `.sidebar-section-title` | `text-[11px] uppercase tracking-[1px] text-content-text-faint px-3 pt-2 pb-1 mb-1` |
| `.sidebar-item` | `flex items-center gap-2 py-[7px] px-2 mx-1 rounded-component cursor-pointer transition-colors duration-fast text-[13px] text-content-text-dim relative` |
| `.sidebar-item:hover` | `hover:bg-surface-bg-3 hover:text-content-text` |
| `.sidebar-item.active` | 用 `<LiquidGlass isActive>` 包裹，内部 `text-brand-jade-glow font-semibold` |
| `.sidebar-footer` | `mt-auto px-3 pb-3 pt-2 border-t border-surface-border` |
| `.sidebar-drag-handle` | `absolute right-[-3px] top-0 bottom-0 w-[6px] cursor-col-resize opacity-0 hover:opacity-100 transition-opacity` |

**重构后的 Sidebar 结构示意**：

```tsx
import LiquidGlass from '@/components/ui/LiquidGlass';

// ... 在模块列表渲染中：
{modules.map((mod, index) => {
  const mId = getModuleId(mod);
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
        className="flex items-center gap-2 py-[7px] px-2 mx-1 rounded-component cursor-pointer transition-colors duration-fast text-[13px]"
        style={isActive ? {} : { color: 'var(--text-dim)' }}
      >
        <span>{mIcon ? <img src={mIcon} alt="" className="w-[18px] h-[18px]" /> : <Square size={18} />}</span>
        <span className="font-mono text-xs">{mName}</span>
      </LiquidGlass>
    </div>
  );
})}
```

**注意**：Quick Access 和 Favorites 中的非 active 项不需要 LiquidGlass，直接用 Tailwind 类即可。

### 4. 更新 `src/components/shell/ShellLayout.tsx`

**改动要点**（最小化）：
- `applyTheme` 调用的默认主题从 `'editorial'` 改为 `'liquid-glass'`
- 如果有 `data-theme="editorial"` 的硬编码引用，改为 `'liquid-glass'`
- 不要改动 ShellLayout 的其他逻辑

## 不要修改的文件

- `tailwind.config.ts`（由 Task A 创建）
- `postcss.config.mjs`（由 Task A 创建）
- `src/context/ThemeContext.tsx`（由 Task B 创建）
- `src/components/ui/LiquidGlass.tsx`（由 Task B 创建）
- `src/components/ui/Modal.tsx`（由 Task B 创建）
- `src/lib/design-tokens.ts`（保留不动）
- 其他组件文件（Header, ContentArea, Terminal 等不在本次范围内）

## 验收标准

1. `pnpm dev` 启动无报错
2. Sidebar 的 active 项有液态玻璃视觉效果（WebGL 或 CSS 降级）
3. Sidebar 的非 active 项使用 Tailwind utility classes 渲染
4. `theme-engine.ts` 中只有 `liquid-glass` 和 `liquid-glass-light` 两个主题
5. `layout.tsx` 使用 `ThemeProvider` 包裹
6. 默认主题为 `liquid-glass`（深色）
7. 可通过修改 `data-theme="liquid-glass-light"` 切换到浅色主题
8. TypeScript 无类型错误

## 输出清单

| 文件 | 操作 |
|------|------|
| `src/lib/theme-engine.ts` | 重写（删除旧主题，新增双主题） |
| `src/app/layout.tsx` | 重写（注入 ThemeProvider） |
| `src/components/shell/Sidebar.tsx` | 重写（Tailwind + LiquidGlass） |
| `src/components/shell/ShellLayout.tsx` | 最小改动（默认主题名） |

## 合并冲突预防

- **Sidebar.tsx** 是本任务独占的，不会有冲突
- **layout.tsx** 是本任务独占的，不会有冲突
- **theme-engine.ts** 是本任务独占的，不会有冲突
- **ShellLayout.tsx** 只改默认主题字符串，改动极小
