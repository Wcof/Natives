# Task A: Tailwind CSS 基础设施 + globals.css 迁移

## 目标

安装 Tailwind CSS 并完成全局样式架构迁移。将现有 `globals.css` 中的纯 CSS 改为 Tailwind utility classes + CSS 变量混合模式，注册所有语义设计令牌。

## 依赖

- 无外部依赖，可独立开始

## 交付物

### 1. 安装依赖

```bash
pnpm add -D tailwindcss@3 postcss autoprefixer
npx tailwindcss init -ts
```

### 2. 创建 `tailwind.config.ts`

路径：项目根目录 `tailwind.config.ts`

```ts
import type { Config } from 'tailwindcss';

const config: Config = {
  content: ['./src/**/*.{ts,tsx}'],
  darkMode: ['class', '[data-theme="liquid-glass"]'],
  theme: {
    extend: {
      colors: {
        brand: {
          olive: '#262920',
          'olive-sidebar': '#1E211A',
          'olive-deep': '#161813',
          'jade-glow': '#F2FFD2',
        },
        // 映射到 CSS 变量，支持主题切换
        surface: {
          bg: 'var(--bg)',
          'bg-2': 'var(--bg-2)',
          'bg-3': 'var(--bg-3)',
          panel: 'var(--panel)',
          border: 'var(--border)',
        },
        content: {
          text: 'var(--text)',
          'text-dim': 'var(--text-dim)',
          'text-faint': 'var(--text-faint)',
        },
        accent: {
          DEFAULT: 'var(--accent)',
          soft: 'var(--accent-soft)',
          ink: 'var(--accent-ink)',
        },
        semantic: {
          danger: 'var(--danger)',
          warning: 'var(--warning)',
          info: 'var(--info)',
        },
      },
      fontFamily: {
        ui: ['var(--font-ui)'],
        mono: ['var(--font-mono)'],
        display: ['var(--font-display)'],
      },
      borderRadius: {
        component: 'var(--radius)',
      },
      boxShadow: {
        'liquid-edge': 'inset 0 1px 1px 0 rgba(255,255,255,0.25), inset 0 -1px 1px 0 rgba(0,0,0,0.3)',
        'glass-ambient': '0 10px 30px -5px rgba(0,0,0,0.25)',
        card: '0 1px 3px rgba(0,0,0,0.12)',
        elevated: '0 4px 12px rgba(0,0,0,0.15)',
        modal: '0 8px 32px rgba(0,0,0,0.2)',
      },
      transitionDuration: {
        fast: '120ms',
        normal: '200ms',
        slow: '300ms',
      },
      transitionTimingFunction: {
        'ease-out-expo': 'cubic-bezier(0.16, 1, 0.3, 1)',
      },
      keyframes: {
        'slide-up': {
          from: { opacity: '0', transform: 'translateY(12px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'fade-in': {
          from: { opacity: '0', transform: 'translateY(4px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'drop-in': {
          from: { opacity: '0', transform: 'scale(0.96)' },
          to: { opacity: '1', transform: 'scale(1)' },
        },
        'live-pulse': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.4' },
        },
        'skeleton-pulse': {
          '0%, 100%': { opacity: '0.4' },
          '50%': { opacity: '0.8' },
        },
      },
      animation: {
        'slide-up': 'slide-up 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
        'fade-in': 'fade-in 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
        'drop-in': 'drop-in 0.16s cubic-bezier(0.2, 0.7, 0.3, 1)',
        'live-pulse': 'live-pulse 1.1s ease-in-out infinite',
        'skeleton-pulse': 'skeleton-pulse 1.5s ease-in-out infinite',
      },
    },
  },
  plugins: [],
};

export default config;
```

### 3. 创建 `postcss.config.mjs`

路径：项目根目录 `postcss.config.mjs`

```js
/** @type {import('postcss-load-config').Config} */
const config = {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
};

export default config;
```

### 4. 重写 `src/app/globals.css`

**关键原则**：
- 文件头部加 `@tailwind base; @tailwind components; @tailwind utilities;`
- `:root` 保留为 CSS 变量声明（Tailwind 通过 `var()` 引用它们）
- 所有组件样式（`.btn`, `.sidebar-item`, `.input` 等）转为 `@apply` 或保留为 `@layer components` 中的类
- 动画 keyframes 保留（Tailwind config 中已注册对应的 animation）
- 两个主题覆盖（`[data-theme="liquid-glass"]` 和 `[data-theme="liquid-glass-light"]`）保留为 CSS

**具体迁移规则**：

| 原 CSS | 迁移方式 |
|--------|----------|
| `:root` 变量 | 保留，新增 liquid-glass 主题变量 |
| `.shell` grid | 转为 `@layer components` 中的 `@apply` |
| `.sidebar` | 转为 `@layer components` 中的 `@apply` |
| `.sidebar-item` | 转为 `@layer components` 中的 `@apply` |
| `.btn`, `.btn-primary`, `.btn-ghost` | 转为 `@layer components` 中的 `@apply` |
| `.input` | 转为 `@layer components` 中的 `@apply` |
| `.context-menu` | 转为 `@layer components` 中的 `@apply` |
| `.glass-overlay` | 转为 `@layer components` 中的 `@apply` |
| 动画 keyframes | 保留原样（globals.css 底部） |
| 主题覆盖 | 新增 `[data-theme="liquid-glass"]` 和 `[data-theme="liquid-glass-light"]` |
| 响应式 `.tb-*` | 保留 |

**新增 liquid-glass 主题变量**（在 `[data-theme="liquid-glass"]` 下）：

```css
[data-theme="liquid-glass"] {
  --bg: #0b0c0a;
  --bg-2: #131410;
  --bg-3: #1c1e17;
  --panel: #1E211A;
  --border: #262920;
  --text: #f2f2ea;
  --text-dim: #9b9d8c;
  --text-faint: #62655a;
  --accent: #cdf24b;
  --accent-soft: #cdf24b1f;
  --accent-ink: #0b0c0a;
  --radius: 4px;
  --danger: #ef4444;
  --warning: #eab308;
  --info: #3b82f6;
  --font-display: var(--font-ui);
}
```

**新增 liquid-glass-light 主题变量**（在 `[data-theme="liquid-glass-light"]` 下）：

```css
[data-theme="liquid-glass-light"] {
  --bg: #f4f1ea;
  --bg-2: #ece8dd;
  --bg-3: #e4dfd2;
  --panel: #f4f1ea;
  --border: #d6d1c3;
  --text: #0a0a0a;
  --text-dim: #57534a;
  --text-faint: #8a857a;
  --accent: #262920;
  --accent-soft: #2629201f;
  --accent-ink: #ffffff;
  --radius: 4px;
  --danger: #dc2626;
  --warning: #d97706;
  --info: #2563eb;
  --font-display: var(--font-ui);
}
```

## 不要修改的文件

- `src/components/` 下任何文件（由 Task C 负责）
- `src/context/` 下任何文件（由 Task B 负责）
- `src/lib/theme-engine.ts`（由 Task C 负责）
- `src/lib/design-tokens.ts`（保留不动，与 Tailwind 共存）
- `src/app/layout.tsx`（由 Task C 负责）
- `package.json`（只加 tailwindcss/postcss/autoprefixer devDependencies）

## 验收标准

1. `pnpm dev` 启动无报错
2. Tailwind utility classes 在任意组件中可用（如 `bg-brand-olive text-brand-jade-glow`）
3. 现有 UI 视觉效果不变（回归检查）
4. `globals.css` 中的 `@tailwind` 指令正确注入
5. 两套主题变量（liquid-glass 深色 + liquid-glass-light 浅色）可通过 `data-theme` 切换
6. 旧主题 `editorial` 和 `warm-archive` 的变量定义保留（向后兼容，后续由 Task C 清理）

## 输出清单

| 文件 | 操作 |
|------|------|
| `tailwind.config.ts` | 新建 |
| `postcss.config.mjs` | 新建 |
| `src/app/globals.css` | 重写 |
| `package.json` | 仅 devDependencies 增加 tailwindcss, postcss, autoprefixer |
