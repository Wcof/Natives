# Agent B 提示词 — ThemeContext + LiquidGlass + Modal 组件

```
你是一个 React 组件工程师。你的任务是为一个 Electron + Next.js 15 + React 19 项目创建 3 个全新的文件：ThemeContext（含 WebGL canvas 看门狗）、LiquidGlass（纯 WebGL shader + CSS 降级）、Modal。

## 项目信息

- 项目路径: /Users/ldh/Downloads/project/AiNative/Natives
- 技术栈: Electron 34 + Next.js 15 (App Router) + React 19 + TypeScript
- 路径别名: @/* 映射到 ./src/*
- 当前样式方案: 纯 CSS 变量（globals.css 中定义），组件用内联 style={{}}
- 没有 Tailwind（另一个 agent 在安装），你的组件不要依赖 Tailwind

## 你的任务（只创建 3 个新文件，不要修改任何现有文件）

### 文件 1: src/context/ThemeContext.tsx

创建 React Context 提供全局主题状态和 WebGL canvas 看门狗。

功能要求：
- 提供 ThemeProvider 组件，包裹 children
- 提供 useTheme() hook，返回 { themeId, setTheme, isDark, registerCanvas, unregisterCanvas, activeCanvasCount }
- themeId: 当前主题 ID（从 document.documentElement.getAttribute('data-theme') 读取）
- setTheme(id): 设置 data-theme 属性，更新 CSS 变量
- isDark: themeId === 'liquid-glass' 为 true，其他为 false
- registerCanvas(): 同步操作，如果当前活跃 canvas 数 < 2 则 +1 返回 true，否则返回 false
- unregisterCanvas(): 同步操作，-1（不低于 0）
- activeCanvasCount: 当前活跃 canvas 数（用于 UI 展示）
- registerCanvas/unregisterCanvas 必须操作 ref（不是 state），避免不必要的重渲染
- activeCanvasCount 用 state（仅用于展示）
- MAX_CONCURRENT_CANVASES = 2

额外导出 useCanvasQuota() hook：
- 返回 { allowed: boolean, release: () => void }
- 内部调用 registerCanvas，如果返回 false 则 allowed=false
- release 调用 unregisterCanvas
- 使用 useRef 跟踪是否已注册，避免重复注册
- 组件卸载时必须调用 release（通过 useEffect cleanup）

类型定义：
interface ThemeContextValue {
  themeId: string;
  setTheme: (id: string) => void;
  isDark: boolean;
  registerCanvas: () => boolean;
  unregisterCanvas: () => void;
  activeCanvasCount: number;
}

### 文件 2: src/components/ui/LiquidGlass.tsx

创建液态玻璃效果组件，使用纯 WebGL2 API + GLSL shader。

接口：
interface LiquidGlassProps {
  isActive: boolean;    // 是否为选中/激活状态
  children: ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

行为：
- isActive=false: 渲染普通 div，透传 className 和 style
- isActive=true: 渲染 ActiveGlass 子组件

ActiveGlass 子组件：
- 通过 useCanvasQuota() 申请 canvas 配额
- allowed=true: 渲染 WebGL canvas + 内容层
- allowed=false: CSS 降级（bg-white/10, mix-blend-plus-lighter, border-white/25, shadow-liquid-edge, text #F2FFD2, font-semibold）

WebGL 实现：
- 使用 WebGL2（getContext('webgl2')）
- Vertex shader: 简单的 quad，传递 texCoord
- Fragment shader: 使用 fBm 噪声模拟液态折射
  - u_time: 动画时间
  - u_resolution: canvas 尺寸
  - u_frost: 霜冻强度 (0.48)
  - u_refraction: 折射强度 (0.028)
  - 基色: vec3(0.149, 0.161, 0.125) 即 #262920
  - 发光色: vec3(0.949, 1.0, 0.824) 即 #F2FFD2
  - 边缘高光模拟 3D 凸起感
  - alpha: 0.85 + frost * 0.1
- Canvas 用 position:absolute inset:0 铺满父容器，pointer-events:none
- 内容用 position:relative z-index:1 叠加在 canvas 上
- requestAnimationFrame 渲染循环
- useEffect cleanup 中 cancelAnimationFrame + 释放 GL 资源
- Canvas 尺寸跟随父容器 resize（getBoundingClientRect + devicePixelRatio）
- 启用 alpha blending: gl.enable(gl.BLEND), gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)

WebGL 初始化失败时降级到 CSS 效果（不报错，静默降级）。

### 文件 3: src/components/ui/Modal.tsx

创建模态弹窗组件。

接口：
interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title?: string;
  children: ReactNode;
  width?: number;        // 默认 480
  showCloseButton?: boolean; // 默认 true
}

功能：
- isOpen=false 返回 null
- 背景: rgba(26, 29, 22, 0.9) 即 #1A1D16 90% + backdrop-filter: blur(24px) saturate(1.5)
- 内容容器: bg var(--bg-2), border rgba(255,255,255,0.15), border-radius var(--radius)
- 标题栏: 有 title 或 showCloseButton 时显示，底部有 border
- 关闭按钮: × 字符，无背景无边框
- Escape 键关闭
- 点击外部关闭（overlay 点击）
- Focus Trap（Tab 循环在弹窗内部）
- 动画: 使用 drop-in 动画（globals.css 中已定义 @keyframes drop-in）
- role="dialog" aria-modal="true" aria-label={title}

## 绝对不要修改的文件

- 任何现有文件都不需要修改
- 只创建 3 个新文件

## 验收标准

1. ThemeContext.tsx 导出 ThemeProvider, useTheme, useCanvasQuota
2. LiquidGlass.tsx 在 isActive=false 时渲染普通 div，在 isActive=true 时渲染 WebGL canvas
3. WebGL canvas 数量不超过 2 个，超出时自动降级到 CSS
4. Modal.tsx 支持 Escape 关闭、点击外部关闭、Focus Trap
5. 所有文件使用 'use client' 指令
6. 所有样式使用内联 style={{}} + CSS 变量（不依赖 Tailwind）
7. TypeScript 无类型错误（运行 tsc --noEmit 验证）
8. LiquidGlass shader 编译无错误

## 与其他任务的接口契约

你的组件会被另一个 agent 在 Sidebar 中使用：
- import { ThemeProvider, useTheme } from '@/context/ThemeContext'
- import LiquidGlass from '@/components/ui/LiquidGlass'
- import Modal from '@/components/ui/Modal'

确保这些导出路径和接口签名严格一致。
```
