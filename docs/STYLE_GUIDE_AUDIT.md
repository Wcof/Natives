# Natives 视觉风格审计与 Fanbox UI 规范对齐方案

本文件是对 Natives 项目视觉风格的深度审计报告。我们通过对比参考项目 Fanbox 的样式系统（`References/fanbox/public/style.css` 及设计 Demo 页面），指出了当前 Natives 在主题覆盖度、排版节奏、原生质感、微动效及终端和谐性上的关键缺口，并给出了具体的修改建议与样式/交互规范。

---

## 🔍 风格审计：Natives vs. Fanbox 关键视觉缺口

通过对比 Natives 的 [globals.css](file:///Users/ldh/Downloads/project/AiNative/Natives/src/app/globals.css) 与 Fanbox 的 `style.css`，我们发现了以下主要视觉与交互缺口：

### 1. 粗野/编辑式主题 (Brutalist Editorial) 缺失
* **现状**: Natives 仅实现了 `terminal-volt` (默认暗色) 和 `warm-archive` (暖色纸感) 两套皮肤。Fanbox 中非常具有特色的第三套主题 `editorial`（参照 Bloomberg Businessweek 的粗野报刊风格）在 Natives 中完全缺失。
* **影响**: 用户无法切换到高对比度、零圆角、带有硬投影和行序号的粗野主义风格。

### 2. 排版字体层级 (Typography) 未与主题绑定
* **现状**: Natives 虽然在 `theme-engine.ts` 中声明了 `--font-display`，但全局及各组件的标题（如侧边栏 Brand、主区 Breadcrumb 最后一级、卡片标题）仍然强行使用统一的无衬线 UI 字体，没有真正消费衬线体（Fraunces/Georgia）或等宽体（JetBrains Mono/SF Mono）。
* **影响**: `warm-archive` 缺乏 Anthropic 式人文出版物的优雅感，`editorial` 缺乏粗野主义的块状冲击力。

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

### A. 三大皮肤配色及设计语境令牌 (Theme Tokens)

| CSS 变量 | 🔴 终端核 (Terminal Volt) | 🟤 暖色档案馆 (Warm Archive) | 📰 编辑式粗野 (Brutalist Editorial) [新] |
| :--- | :--- | :--- | :--- |
| `--bg` | `#0b0c0a` (极深黑) | `#f5f0e8` (暖白纸色) | `#f4f1ea` (灰黄报纸色) |
| `--bg-2` | `#131410` (暗墨绿) | `#ece4d6` (卡夫纸色) | `#ece8dd` (暗灰黄) |
| `--bg-3` | `#1c1e17` (中墨绿) | `#e5dccd` (深卡夫纸色) | `#e4dfd2` (灰黄边框色) |
| `--panel` | `#0e0f0c` (侧栏底色) | `#efe9df` (温暖侧栏底色) | `#f4f1ea` (与底色一致) |
| `--border` | `#262920` (暗绿缝线) | `#d4c9b7` (灰褐缝线) | `#d6d1c3` (冷灰缝线) |
| `--text` | `#f2f2ea` (亮绿白) | `#2c2418` (浓浓墨色) | `#0a0a0a` (纯黑) |
| `--text-dim` | `#9b9d8c` (暗绿灰) | `#6b5f4e` (中墨色) | `#57534a` (深灰) |
| `--text-faint`| `#62655a` (灰绿) | `#a0947d` (浅墨色) | `#8a857a` (浅灰) |
| `--accent` | `#cdf24b` (荧光黄绿) | `#cc785c` (红土陶色) | `#ff433d` (经典印刷红) |
| `--accent-ink`| `#0b0c0a` (反白黑) | `#f5f0e8` (反白纸色) | `#ffffff` (反白纯白) |
| `--radius` | `4px` | `9px` | `0px` |
| `--shadow` | `0 10px 40px rgba(0,0,0,0.6)`| `0 14px 44px rgba(90,66,42,0.18)`| `10px 10px 0 rgba(10,10,10,0.9)` (硬投影) |
| `--font-display`| `ui-monospace, monospace` | `"Fraunces", Georgia, serif` | `var(--font-ui)` (加粗 800) |

---

### B. 排版与字体绑定 (Typography Binding)

为实现不同语境下的排版呼吸感，各组件需严格绑定字体变量：
1. **主标题、侧栏标志、面包屑激活态**:
   `font-family: var(--font-display);`
   - 当激活 `warm-archive` 时，呈现典雅的衬线大字。
   - 当激活 `editorial` 时，呈现极粗、间距紧密的 sans-serif 块状字。
2. **代码文件名称、列表数值、终端数据**:
   `font-family: var(--font-mono);`
   - 使用统一的等宽字体对齐，方便开发者阅读。

---

### C. macOS 原生质感与微动效 (Vibe & Motion)

1. **纸张肌理 (Paper Texture)**:
   在 `[data-theme="warm"]` 的 body 上，必须追加以下背景样式：
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

1. **补全 `theme-engine.ts`**：
   在 `THEMES` 中添加 `'editorial'` 主题，在 `TERMINAL_THEMES` 中添加 `'editorial'` 的终端背景配色。
2. **更新 `globals.css`**：
   - 写入 `:focus-visible` 重置规则，清除鼠标点击时的默认粗边框，只在键盘 focus 时提供精致的 `--accent` 边框。
   - 编写 `[data-theme="editorial"]` 特殊样式规则（例如重置 `.list .row` 的圆角为 `0px`，边框线加粗，列表头部变为纯黑底反白字等）。
   - 补全 `warm-archive` 的背景肌理。
3. **绑定字体变量**：
   更新组件（例如 `Sidebar.tsx` 的 Brand，`FileBrowser.tsx` 的面包屑和文件名）使用相应的 `var(--font-display)` 或 `var(--font-mono)`。
4. **终端 ANSI 颜色集成**：
   重构 `Terminal.tsx`，当监听到主题变更时，动态将 xterm 的 `theme` 变更为来自 `theme-engine.ts` 中对应的 `TERMINAL_THEMES`。
