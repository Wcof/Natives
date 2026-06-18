# Natives 视觉风格审计与 Fanbox UI 规范对齐方案

本文件是对 Natives 项目视觉风格的深度审计报告。我们通过对比参考项目 Fanbox 的样式系统（`References/fanbox/public/style.css` 及设计 Demo 页面），指出了当前 Natives 在主题覆盖度、排版节奏、原生质感、微动效及终端和谐性上的关键缺口，并给出了具体的修改建议与样式/交互规范。

---

## 🔍 风格审计：Natives vs. Fanbox 关键视觉缺口

通过对比 Natives 的 [globals.css](file:///Users/ldh/Downloads/project/AiNative/Natives/src/app/globals.css) 与 Fanbox 的 `style.css`，我们发现了以下主要视觉与交互缺口：

### 1. 主题精简完成（Editorial 已移除，Warm Archive 已重命名为磨砂茉莉）
* **现状**: Natives 已从原先的 `terminal-volt` (深色) + `warm-archive` (暖色纸感) 精简为 `terminal-volt` (深色伏特) + `frosted-jasmine` (浅色磨砂茉莉) 两套皮肤。原先计划的第三套 `editorial`（粗野报刊风格）已被移除。
* **影响**: 用户可在深色磨砂玻璃与浅色磨砂玻璃间切换。

### 2. 排版字体层级 (Typography) 未与主题绑定
* **现状**: Natives 虽然在 `theme-engine.ts` 中声明了 `--font-display`，但全局及各组件的标题（如侧边栏 Brand、主区 Breadcrumb 最后一级、卡片标题）仍然强行使用统一的无衬线 UI 字体，没有真正消费衬线体（Fraunces/Georgia）或等宽体（JetBrains Mono/SF Mono）。
* **影响**: `frosted-jasmine` 获得了雅致衬线感，`terminal-volt` 获得终端等宽感。

### 3. macOS 原生细节与交互线索粗糙
* **现状**: 
  - **侧边栏拖拽条 (Resizer)**: Natives 的 Resizer 仅在拖拽时有纯颜色变化，缺少 Fanbox 细致的 hover 渐显光晕、手势光标 (`col-resize`) 以及跟手惯性。
  - **卡片与边框**: 缺少 macOS 风格的内阴影（`inset 0 1px 0 rgba(255, 255, 255, 0.15)`）和多层柔和投影，显得平淡和有“网页套壳味”。
  - **活动指示器**: 侧边栏的活动条没有像 Fanbox 那样使用左侧伸出红线/琥珀色线（`::before`）的交互暗示，选中态仅使用浅底色。

### 4. 键盘焦点可达性 (Focus ring) 未降噪
* **现状**: Natives 缺乏对 `:focus-visible` 的控制，键盘焦点和鼠标点击焦点混杂，边缘焦点环生硬，不符合高品质 macOS 应用的无摩擦直觉。

### 5. 暖色纸张纹理 (Texture) 缺失
* **现状**: Fanbox 的暖色档案主题在 body 挂载了高精度的 radial-gradient 浅褐色墨点网格和 SVG 湍流噪声（Turbulence），以模拟粗糙的再生纸张感。Natives 的暖色主题仅为纯色底，缺乏质感。

### 6. 交互微动效 (Micro-interactions) 缺失
* **现状**: 布局拉伸、Iframe 滑入（`edIn` 动效）、拖拽文件进终端提示区等均缺少缓动动画过渡。

---

## 🎨 样式与交互规范定义 (Style & Interaction Specs)

执行 Agent 应当根据以下规范更新 `/Users/ldh/Downloads/project/AiNative/Natives` 中的样式代码：

### A. 二皮肤配色及设计语境令牌 (Theme Tokens)

| CSS 变量 | 🔴 终端核 (Terminal Volt) | 🟢 磨砂茉莉 (Frosted Jasmine) |
| :--- | :--- | :--- |
| `--bg` | `#0d0f12` (极深色) | `#fdf6f0` (暖白茉莉色) |
| `--bg-2` | `#15181d` (暗钢蓝) | `#f9eee4` (暖杏色) |
| `--bg-3` | `#1c2027` (中钢蓝) | `#f5e5d6` (暖卡夫色) |
| `--panel` | `#0d0f12` (面板底色) | `#fcf3ea` (暖面板色) |
| `--border` | `#2a2f38` (暗缝线) | `#e8d5c0` (褐缝线) |
| `--text` | `#d4d7de` (亮灰白) | `#2d1f14` (浓浓墨色) |
| `--text-dim` | `#8b90a0` (暗灰) | `#7a6b5a` (中墨色) |
| `--text-faint`| `#555a66` (灰) | `#b8a594` (浅墨色) |
| `--accent` | `#00ff9c` (伏特绿) | `#7a9e7e` (鼠尾草绿) |
| `--accent-ink`| `#0d0f12` (反白黑) | `#fdf6f0` (反白纸色) |
| `--radius` | `4px` | `12px` |
| `--font-display`| `"JetBrains Mono", monospace` | `"Noto Serif SC", Georgia, serif` |

---

### B. 排版与字体绑定 (Typography Binding)

为实现不同语境下的排版呼吸感，各组件需严格绑定字体变量：
1. **主标题、侧栏标志、面包屑激活态**:
   `font-family: var(--font-display);`
    - 当激活 `frosted-jasmine` 时，呈现典雅的衬线大字。
    - 当激活 `terminal-volt` 时，呈现等宽终端感。
2. **代码文件名称、列表数值、终端数据**:
   `font-family: var(--font-mono);`
   - 使用统一的等宽字体对齐，方便开发者阅读。

---

### C. macOS 原生质感与微动效 (Vibe & Motion)

1. **纸张肌理 (Paper Texture)**:
   在 `[data-theme="frosted-jasmine"]` 的 body 上，追加以下背景样式（已实现）：
   ```css
   [data-theme="warm"] body {
     background-image: radial-gradient(rgba(140, 110, 70, 0.05) 1px, transparent 1px);
     background-size: 4px 4px;
   }
   ```
2. **拖拽条动画 (Sidebar Drag Handle)**:
   ```css
   .sidebar-drag-handle {
     position: absolute; right: -3px; top: 0; bottom: 0; width: 6px; cursor: col-resize; z-index: var(--z-dragbar);
   }
   .sidebar-drag-handle::after {
     content: ''; position: absolute; inset: 0 2px; border-radius: 1px; background: var(--accent); opacity: 0;
     transition: opacity 0.15s ease;
   }
   .sidebar-drag-handle:hover::after, .sidebar-drag-handle.active::after {
     opacity: 0.55;
   }
   ```
3. **滑入动画 (Slide-in / Fade-in)**:
   面板切换、Iframe 打开及命令面板唤起时，应用 `cubic-bezier(0.16, 1, 0.3, 1)` 物理缓动曲线，例如：
   ```css
   @keyframes edIn {
     from { opacity: 0; transform: translateY(8px); }
     to { opacity: 1; transform: translateY(0); }
   }
   .animate-fade-in {
     animation: edIn 0.24s cubic-bezier(0.16, 1, 0.3, 1) forwards;
   }
   ```

---

## 🛠️ 具体改造实施清单

请执行 Agent 按照以下步骤修改代码：

1. **已完成 ~ `theme-engine.ts`**：
   二皮肤 `frosted-jasmine` + `terminal-volt` 已定义，`editorial` 已移除。
2. **已完成 ~ `globals.css`**：
   - `:focus-visible` 重置规则已添加。
   - `[data-theme="frosted-jasmine"]` 特殊样式（纸张肌理）已实现。
   - ediorial 相关样式已移除。
3. **已完成 ~ 字体变量绑定**：
   相关组件已使用 `var(--font-display)` 等主题变量。
4. **已完成 ~ 终端 ANSI 颜色集成**：
   `theme-engine.ts` 中 `TERMINAL_THEMES` 已覆盖二皮肤。
