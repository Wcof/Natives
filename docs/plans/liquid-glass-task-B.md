# Task B: ThemeContext + LiquidGlass + Modal 组件

## 目标

创建 3 个新文件：React 19 ThemeContext（含 WebGL canvas 看门狗）、LiquidGlass 组件（纯 WebGL shader + CSS 降级）、Modal 组件。这 3 个组件是完全自包含的，不修改任何现有文件。

## 依赖

- 无外部依赖，可独立开始
- 不需要 Tailwind（组件用内联样式 + CSS 变量）
- 不需要修改任何现有文件

## 交付物

### 1. `src/context/ThemeContext.tsx`

**职责**：
- 提供全局主题状态（当前主题 ID、深色/浅色判断）
- 提供 WebGL canvas 注册看门狗（最多 2 个并发 canvas）
- 提供 `useTheme()` hook
- 提供 `useCanvasQuota()` hook（LiquidGlass 组件用）

**接口定义**：

```tsx
'use client';

import { createContext, useContext, useState, useCallback, useRef, type ReactNode } from 'react';

interface ThemeContextValue {
  /** 当前主题 ID */
  themeId: string;
  /** 设置主题（更新 data-theme 属性 + CSS 变量） */
  setTheme: (id: string) => void;
  /** 当前是否深色主题 */
  isDark: boolean;
  /** 注册一个 WebGL canvas，返回 true 表示允许，false 表示超限（降级到 CSS） */
  registerCanvas: () => boolean;
  /** 注销一个 WebGL canvas */
  unregisterCanvas: () => void;
  /** 当前活跃 canvas 数量 */
  activeCanvasCount: number;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

const MAX_CONCURRENT_CANVASES = 2;

export function ThemeProvider({ children, defaultTheme = 'liquid-glass' }: {
  children: ReactNode;
  defaultTheme?: string;
}) {
  // 实现要点：
  // 1. themeId state，初始化从 document.documentElement.getAttribute('data-theme') 读取
  // 2. setTheme: 设置 data-theme 属性，更新 CSS 变量（复用 theme-engine.ts 的逻辑）
  // 3. isDark: 基于 themeId 判断（liquid-glass = dark, liquid-glass-light = light）
  // 4. canvasCount ref（用 ref 而非 state，避免不必要的重渲染）
  // 5. registerCanvas: count < MAX 则 ++count 返回 true，否则返回 false
  // 6. unregisterCanvas: --count（不低于 0）
  // 7. activeCanvasCount state（仅用于 UI 展示，register/unregister 时同步更新）
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error('useTheme must be used within ThemeProvider');
  return ctx;
}

/**
 * LiquidGlass 组件专用 hook。
 * 返回 { allowed: boolean, release: () => void }
 * 组件挂载时调用 register，卸载时调用 release。
 */
export function useCanvasQuota(): { allowed: boolean; release: () => void } {
  const { registerCanvas, unregisterCanvas } = useTheme();
  const registered = useRef(false);

  // 组件首次渲染时尝试注册
  // 注意：不能在 render 中直接 setState，需要用 useEffect
  // 但 LiquidGlass 需要在渲染时就知道是否 allowed
  // 解决方案：registerCanvas 是同步的（操作 ref），不需要 useEffect

  if (!registered.current) {
    const allowed = registerCanvas();
    registered.current = true;
    // 如果不允许，立即注销（不占用名额）
    if (!allowed) {
      unregisterCanvas();
      registered.current = false;
      return { allowed: false, release: () => {} };
    }
  }

  return {
    allowed: true,
    release: () => {
      if (registered.current) {
        unregisterCanvas();
        registered.current = false;
      }
    },
  };
}
```

**重要实现细节**：
- `registerCanvas` 和 `unregisterCanvas` 必须是同步操作（操作 ref），不能是异步的
- `useCanvasQuota` 在组件卸载时必须调用 `release`（通过 useEffect cleanup）
- `isDark` 的判断逻辑：`themeId === 'liquid-glass'` 为 dark，`themeId === 'liquid-glass-light'` 为 light
- ThemeProvider 需要在客户端挂载时读取 `document.documentElement.getAttribute('data-theme')` 初始化 themeId

### 2. `src/components/ui/LiquidGlass.tsx`

**职责**：
- 接收 `isActive` prop，当 active 时渲染 WebGL 液态玻璃效果
- 通过 `useCanvasQuota()` 向 ThemeProvider 申请 canvas 配额
- 配额不足时自动降级到纯 CSS 效果
- 非 active 时渲染普通样式

**接口定义**：

```tsx
'use client';

import { useRef, useEffect, useCallback, type ReactNode } from 'react';
import { useCanvasQuota } from '@/context/ThemeContext';

interface LiquidGlassProps {
  /** 是否为选中/激活状态 */
  isActive: boolean;
  /** 子元素 */
  children: ReactNode;
  /** 可选的 className */
  className?: string;
  /** 可选的内联样式 */
  style?: React.CSSProperties;
}

export default function LiquidGlass({ isActive, children, className, style }: LiquidGlassProps) {
  // ── 非 active 状态：普通渲染 ──
  if (!isActive) {
    return (
      <div className={className} style={style}>
        {children}
      </div>
    );
  }

  // ── Active 状态：尝试 WebGL，失败则 CSS 降级 ──
  return <ActiveGlass className={className} style={style}>{children}</ActiveGlass>;
}

function ActiveGlass({ children, className, style }: { children: ReactNode; className?: string; style?: React.CSSProperties }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const glRef = useRef<WebGL2RenderingContext | null>(null);
  const programRef = useRef<WebGLProgram | null>(null);
  const animFrameRef = useRef<number>(0);
  const { allowed, release } = useCanvasQuota();

  useEffect(() => {
    if (!allowed) return () => release();

    const canvas = canvasRef.current;
    if (!canvas) return () => release();

    // ── 初始化 WebGL2 ──
    const gl = canvas.getContext('webgl2', {
      alpha: true,
      premultipliedAlpha: false,
      antialias: true,
    });

    if (!gl) {
      // WebGL 不可用，降级
      release();
      return () => {};
    }

    glRef.current = gl;

    // ── Shader 源码 ──
    const vertexShaderSource = `#version 300 es
      in vec2 a_position;
      in vec2 a_texCoord;
      out vec2 v_texCoord;
      void main() {
        gl_Position = vec4(a_position, 0.0, 1.0);
        v_texCoord = a_texCoord;
      }
    `;

    const fragmentShaderSource = `#version 300 es
      precision highp float;

      in vec2 v_texCoord;
      out vec4 fragColor;

      uniform float u_time;
      uniform vec2 u_resolution;
      uniform float u_frost;
      uniform float u_refraction;

      // Simplex noise 或简单 sin 组合模拟液态折射
      float hash(vec2 p) {
        return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
      }

      float noise(vec2 p) {
        vec2 i = floor(p);
        vec2 f = fract(p);
        f = f * f * (3.0 - 2.0 * f);
        float a = hash(i);
        float b = hash(i + vec2(1.0, 0.0));
        float c = hash(i + vec2(0.0, 1.0));
        float d = hash(i + vec2(1.0, 1.0));
        return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
      }

      float fbm(vec2 p) {
        float v = 0.0;
        float a = 0.5;
        for (int i = 0; i < 4; i++) {
          v += a * noise(p);
          p *= 2.0;
          a *= 0.5;
        }
        return v;
      }

      void main() {
        vec2 uv = v_texCoord;
        vec2 pixel = uv * u_resolution;

        // 液态折射扰动
        float n = fbm(uv * 3.0 + u_time * 0.15);
        float distortion = n * u_refraction;

        // 玻璃霜冻效果
        float frost = fbm(uv * 8.0 + u_time * 0.05) * u_frost;

        // 边缘光（3D 凸起感）
        float edgeLight = smoothstep(0.0, 0.02, uv.y) * smoothstep(1.0, 0.98, uv.y);
        float edgeDark = smoothstep(0.0, 0.02, uv.x) * smoothstep(1.0, 0.98, uv.x);
        float edge = edgeLight * edgeDark;

        // 基色：半透明橄榄绿
        vec3 baseColor = vec3(0.149, 0.161, 0.125); // #262920
        vec3 glowColor = vec3(0.949, 1.0, 0.824);   // #F2FFD2

        // 混合
        vec3 color = mix(baseColor, glowColor, frost * 0.12 + distortion * 0.05);
        color += vec3(0.15) * edge; // 边缘高光

        // 内发光边缘
        float innerGlow = smoothstep(0.0, 0.05, uv.x) * smoothstep(1.0, 0.95, uv.x) *
                          smoothstep(0.0, 0.05, uv.y) * smoothstep(1.0, 0.95, uv.y);
        color += glowColor * 0.03 * (1.0 - innerGlow);

        float alpha = 0.85 + frost * 0.1;
        fragColor = vec4(color, alpha);
      }
    `;

    // ── 编译 Shader ──
    function createShader(type: number, source: string): WebGLShader | null {
      const shader = gl.createShader(type)!;
      gl.shaderSource(shader, source);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        console.error('Shader compile error:', gl.getShaderInfoLog(shader));
        gl.deleteShader(shader);
        return null;
      }
      return shader;
    }

    const vs = createShader(gl.VERTEX_SHADER, vertexShaderSource);
    const fs = createShader(gl.FRAGMENT_SHADER, fragmentShaderSource);
    if (!vs || !fs) {
      release();
      return () => {};
    }

    const program = gl.createProgram()!;
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.error('Program link error:', gl.getProgramInfoLog(program));
      release();
      return () => {};
    }

    programRef.current = program;
    gl.useProgram(program);

    // ── 顶点数据 ──
    const positions = new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]);
    const texCoords = new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]);

    const posBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, posBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);
    const posLoc = gl.getAttribLocation(program, 'a_position');
    gl.enableVertexAttribArray(posLoc);
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

    const texBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, texBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, texCoords, gl.STATIC_DRAW);
    const texLoc = gl.getAttribLocation(program, 'a_texCoord');
    gl.enableVertexAttribArray(texLoc);
    gl.vertexAttribPointer(texLoc, 2, gl.FLOAT, false, 0, 0);

    // ── Uniforms ──
    const uTime = gl.getUniformLocation(program, 'u_time');
    const uResolution = gl.getUniformLocation(program, 'u_resolution');
    const uFrost = gl.getUniformLocation(program, 'u_frost');
    const uRefraction = gl.getUniformLocation(program, 'u_refraction');

    // ── 渲染循环 ──
    const startTime = performance.now();

    function render() {
      const canvas = canvasRef.current;
      if (!canvas || !gl) return;

      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const w = rect.width * dpr;
      const h = rect.height * dpr;

      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }

      gl.viewport(0, 0, w, h);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

      const elapsed = (performance.now() - startTime) / 1000;

      gl.uniform1f(uTime, elapsed);
      gl.uniform2f(uResolution, w, h);
      gl.uniform1f(uFrost, 0.48);
      gl.uniform1f(uRefraction, 0.028);

      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

      animFrameRef.current = requestAnimationFrame(render);
    }

    animFrameRef.current = requestAnimationFrame(render);

    return () => {
      cancelAnimationFrame(animFrameRef.current);
      gl.deleteProgram(program);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      gl.deleteBuffer(posBuffer);
      gl.deleteBuffer(texBuffer);
      release();
    };
  }, [allowed, release]);

  // ── CSS 降级（WebGL 配额不足或不可用）──
  if (!allowed) {
    return (
      <div
        className={className}
        style={{
          ...style,
          background: 'rgba(255, 255, 255, 0.1)',
          mixBlendMode: 'plus-lighter',
          border: '1px solid rgba(255, 255, 255, 0.25)',
          boxShadow: 'inset 0 1px 1px 0 rgba(255,255,255,0.25), inset 0 -1px 1px 0 rgba(0,0,0,0.3)',
          color: '#F2FFD2',
          fontWeight: 600,
        }}
      >
        {children}
      </div>
    );
  }

  // ── WebGL 渲染 ──
  return (
    <div className={className} style={{ position: 'relative', ...style }}>
      <canvas
        ref={canvasRef}
        style={{
          position: 'absolute',
          inset: 0,
          width: '100%',
          height: '100%',
          borderRadius: 'inherit',
          pointerEvents: 'none',
        }}
      />
      <div style={{ position: 'relative', zIndex: 1, color: '#F2FFD2', fontWeight: 600 }}>
        {children}
      </div>
    </div>
  );
}
```

### 3. `src/components/ui/Modal.tsx`

**职责**：
- 可复用的模态弹窗组件
- 毛玻璃背景 + 键盘导航（Escape 关闭、Focus Trap）
- 不使用 WebGL（节省 canvas 配额）

**接口定义**：

```tsx
'use client';

import { useEffect, useRef, type ReactNode } from 'react';

interface ModalProps {
  /** 是否显示 */
  isOpen: boolean;
  /** 关闭回调 */
  onClose: () => void;
  /** 标题 */
  title?: string;
  /** 子内容 */
  children: ReactNode;
  /** 宽度（默认 480px） */
  width?: number;
  /** 是否显示关闭按钮（默认 true） */
  showCloseButton?: boolean;
}

export default function Modal({
  isOpen,
  onClose,
  title,
  children,
  width = 480,
  showCloseButton = true,
}: ModalProps) {
  const overlayRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  // Escape 键关闭
  useEffect(() => {
    if (!isOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [isOpen, onClose]);

  // Focus trap
  useEffect(() => {
    if (!isOpen || !contentRef.current) return;
    const focusable = contentRef.current.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    );
    if (focusable.length > 0) focusable[0]!.focus();

    const trapFocus = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || focusable.length === 0) return;
      const first = focusable[0]!;
      const last = focusable[focusable.length - 1]!;
      if (e.shiftKey) {
        if (document.activeElement === first) { e.preventDefault(); last.focus(); }
      } else {
        if (document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    };
    document.addEventListener('keydown', trapFocus);
    return () => document.removeEventListener('keydown', trapFocus);
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div
      ref={overlayRef}
      onClick={(e) => { if (e.target === overlayRef.current) onClose(); }}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 60,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'rgba(26, 29, 22, 0.9)', // #1A1D16 at 90%
        backdropFilter: 'blur(24px) saturate(1.5)',
      }}
    >
      <div
        ref={contentRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        style={{
          width,
          maxWidth: '90vw',
          maxHeight: '85vh',
          overflow: 'auto',
          background: 'var(--bg-2, #131410)',
          border: '1px solid rgba(255, 255, 255, 0.15)',
          borderRadius: 'var(--radius, 4px)',
          boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
          animation: 'drop-in 0.16s cubic-bezier(0.2, 0.7, 0.3, 1)',
        }}
      >
        {(title || showCloseButton) && (
          <div style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '12px 16px',
            borderBottom: '1px solid var(--border, #262920)',
          }}>
            {title && (
              <h2 style={{
                margin: 0,
                fontSize: 14,
                fontWeight: 600,
                color: 'var(--text, #f2f2ea)',
              }}>
                {title}
              </h2>
            )}
            {showCloseButton && (
              <button
                onClick={onClose}
                aria-label="Close"
                style={{
                  background: 'none',
                  border: 'none',
                  color: 'var(--text-faint, #62655a)',
                  cursor: 'pointer',
                  padding: 4,
                  fontSize: 18,
                  lineHeight: 1,
                }}
              >
                ×
              </button>
            )}
          </div>
        )}
        <div style={{ padding: 16 }}>
          {children}
        </div>
      </div>
    </div>
  );
}
```

## 不要修改的文件

- 任何现有文件都不需要修改
- 只创建 3 个新文件

## 验收标准

1. `ThemeContext.tsx` 导出 `ThemeProvider` 和 `useTheme` 和 `useCanvasQuota`
2. `LiquidGlass.tsx` 在 `isActive=false` 时渲染普通 div，在 `isActive=true` 时渲染 WebGL canvas
3. WebGL canvas 数量不超过 2 个，超出时自动降级到 CSS
4. `Modal.tsx` 支持 Escape 关闭、点击外部关闭、Focus Trap
5. 所有文件使用 `'use client'` 指令
6. 所有样式使用内联 `style={{}}` + CSS 变量（不依赖 Tailwind）
7. TypeScript 无类型错误

## 输出清单

| 文件 | 操作 |
|------|------|
| `src/context/ThemeContext.tsx` | 新建 |
| `src/components/ui/LiquidGlass.tsx` | 新建 |
| `src/components/ui/Modal.tsx` | 新建 |

## 与其他任务的接口契约

### 供 Task C 使用

Task C（Sidebar + 集成）需要：
- `import { ThemeProvider, useTheme } from '@/context/ThemeContext'` — 在 layout.tsx 中包裹
- `import LiquidGlass from '@/components/ui/LiquidGlass'` — 在 Sidebar 中包裹 active 项
- `import Modal from '@/components/ui/Modal'` — 替换现有 ConfirmDialog 或新增使用

### 接收来自 Task A 的 CSS 变量

LiquidGlass 的 CSS 降级模式使用以下 CSS 变量（由 Task A 在 globals.css 中定义）：
- `var(--bg-2)` — 背景色
- `var(--border)` — 边框色
- `var(--text)` — 文字色

这些变量在当前 `globals.css` 的 `:root` 中已经存在，Task A 不会删除它们，所以即使 Task A 尚未完成，本任务的组件也能正常工作。
