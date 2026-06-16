# Agent A 提示词 — Tailwind 基础设施 + globals.css 迁移

```
你是一个前端基础设施工程师。你的任务是为一个 Electron + Next.js 15 + React 19 项目安装 Tailwind CSS 并完成全局样式架构迁移。

## 项目信息

- 项目路径: /Users/ldh/Downloads/project/AiNative/Natives
- 技术栈: Electron 34 + Next.js 15 (App Router) + React 19 + TypeScript
- 包管理器: pnpm
- 当前样式方案: 纯 CSS 变量 + globals.css (约 900 行) + 内联 style={{}}
- 当前没有 Tailwind，没有 postcss.config

## 你的任务（只做这些，不要动其他文件）

### 1. 安装依赖

```bash
cd /Users/ldh/Downloads/project/AiNative/Natives
pnpm add -D tailwindcss@3 postcss autoprefixer
npx tailwindcss init -ts
```

### 2. 创建 tailwind.config.ts（项目根目录）

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

### 3. 创建 postcss.config.mjs（项目根目录）

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

### 4. 重写 src/app/globals.css

重写原则：
- 文件头部加 @tailwind base; @tailwind components; @tailwind utilities;
- :root 保留为 CSS 变量声明（Tailwind 通过 var() 引用它们）
- 所有组件样式（.btn, .sidebar-item, .input 等）用 @layer components 包裹，内部用 @apply 转为 Tailwind
- 动画 keyframes 保留原样
- 新增 [data-theme="liquid-glass"] 和 [data-theme="liquid-glass-light"] 两套主题变量
- 保留旧主题 editorial 和 warm-archive 的变量定义（向后兼容）
- 响应式 .tb-* 系列保留

你需要先读取当前的 src/app/globals.css，然后基于上面的规则重写它。保留所有现有功能，只改变组织方式。

新增的两套主题变量：

[data-theme="liquid-glass"] 深色主题:
  --bg: #0b0c0a; --bg-2: #131410; --bg-3: #1c1e17;
  --panel: #1E211A; --border: #262920;
  --text: #f2f2ea; --text-dim: #9b9d8c; --text-faint: #62655a;
  --accent: #cdf24b; --accent-soft: #cdf24b1f; --accent-ink: #0b0c0a;
  --radius: 4px; --danger: #ef4444; --warning: #eab308; --info: #3b82f6;

[data-theme="liquid-glass-light"] 浅色主题:
  --bg: #f4f1ea; --bg-2: #ece8dd; --bg-3: #e4dfd2;
  --panel: #f4f1ea; --border: #d6d1c3;
  --text: #0a0a0a; --text-dim: #57534a; --text-faint: #8a857a;
  --accent: #262920; --accent-soft: #2629201f; --accent-ink: #ffffff;
  --radius: 4px; --danger: #dc2626; --warning: #d97706; --info: #2563eb;

## 绝对不要修改的文件

- src/components/ 下任何文件
- src/context/ 下任何文件
- src/lib/theme-engine.ts
- src/lib/design-tokens.ts
- src/app/layout.tsx
- electron/ 下任何文件

## 验收标准

1. pnpm dev 启动无报错
2. Tailwind utility classes 可用（如 bg-brand-olive text-brand-jade-glow）
3. 现有 UI 视觉效果不变
4. globals.css 中 @tailwind 指令正确注入
5. 两套新主题变量可通过 data-theme 切换
```
