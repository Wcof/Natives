# Liquid Glass 重构 — 三任务合并指南

## 任务概览

| 任务 | 负责文件 | 依赖 | 可并行 |
|------|----------|------|--------|
| **Task A** — Tailwind 基础设施 | `tailwind.config.ts`, `postcss.config.mjs`, `globals.css` | 无 | ✅ 立即开始 |
| **Task B** — 新组件 | `src/context/ThemeContext.tsx`, `src/components/ui/LiquidGlass.tsx`, `src/components/ui/Modal.tsx` | 无 | ✅ 立即开始 |
| **Task C** — 集成层 | `theme-engine.ts`, `layout.tsx`, `Sidebar.tsx`, `ShellLayout.tsx` | A + B | ⏳ 等 A、B 完成后开始 |

## 合并顺序

```
1. 先合并 Task A（Tailwind 基础）
2. 再合并 Task B（新组件，无冲突）
3. 最后合并 Task C（集成层，依赖 A+B 的产出）
```

## 文件冲突矩阵

| 文件 | Task A | Task B | Task C | 冲突风险 |
|------|--------|--------|--------|----------|
| `tailwind.config.ts` | ✅ 创建 | — | — | 无 |
| `postcss.config.mjs` | ✅ 创建 | — | — | 无 |
| `globals.css` | ✅ 重写 | — | — | 无 |
| `package.json` | 🔧 改 devDeps | — | — | 无 |
| `src/context/ThemeContext.tsx` | — | ✅ 创建 | — | 无 |
| `src/components/ui/LiquidGlass.tsx` | — | ✅ 创建 | — | 无 |
| `src/components/ui/Modal.tsx` | — | ✅ 创建 | — | 无 |
| `src/lib/theme-engine.ts` | — | — | ✅ 重写 | 无 |
| `src/app/layout.tsx` | — | — | ✅ 重写 | 无 |
| `src/components/shell/Sidebar.tsx` | — | — | ✅ 重写 | 无 |
| `src/components/shell/ShellLayout.tsx` | — | — | 🔧 小改 | 无 |

**结论：3 个任务修改的文件完全不重叠，零冲突风险。**

## 合并后验证步骤

### Step 1: 安装依赖
```bash
cd /Users/ldh/Downloads/project/AiNative/Natives
pnpm install  # 安装 tailwindcss, postcss, autoprefixer
```

### Step 2: TypeScript 类型检查
```bash
rtk tsc
```

预期：无错误。如果 Task B 的 `ThemeContext.tsx` 导出类型与 Task C 的 import 不匹配，此处会报错。

### Step 3: 开发服务器启动
```bash
pnpm dev
```

预期：无编译错误，浏览器可访问。

### Step 4: 视觉验证清单

- [ ] 默认主题为 liquid-glass（深色橄榄绿背景）
- [ ] Sidebar 的 active 项有液态玻璃效果（或 CSS 降级效果）
- [ ] Sidebar 的非 active 项正常显示（hover 有反馈）
- [ ] 可通过 DevTools 修改 `data-theme="liquid-glass-light"` 切换浅色主题
- [ ] 浅色主题下所有文字可读、对比度足够
- [ ] Modal 组件可通过手动测试验证（如果有使用场景）
- [ ] 终端面板正常工作
- [ ] Header 正常工作
- [ ] 页面切换无闪烁

### Step 5: 构建验证
```bash
rtk next build
```

预期：构建成功，无错误。

## 回滚方案

如果合并后出现严重问题：

```bash
# 丢弃所有改动
git checkout -- .

# 或者逐个回滚
git checkout -- src/components/shell/Sidebar.tsx  # 回滚 Task C
git checkout -- src/context/ src/components/ui/LiquidGlass.tsx src/components/ui/Modal.tsx  # 回滚 Task B
git checkout -- tailwind.config.ts postcss.config.mjs src/app/globals.css  # 回滚 Task A
```

## 后续优化（不在本次范围内）

1. **其他组件 Tailwind 迁移** — Header, ContentArea, RightPanel, Terminal 等
2. **design-tokens.ts 对齐** — 将 JS tokens 对齐到 Tailwind config
3. **WebGL 性能调优** — 根据实际设备调整 shader 参数
4. **主题设置 UI** — 在 SettingsPage 中添加主题切换控件
5. **ConfirmDialog 升级** — 使用 Modal 组件替换现有实现
