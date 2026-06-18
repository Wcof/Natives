# 前端架构 01 · 目录、命名与组件分层

> **版本**: 1.0.0 · **日期**: 2026-06-15
> **关联 ADR**: 无
> **关联源文件**: `src/components/`、`src/app/`、`src/lib/`、`src/hooks/`

---

## 一、本篇要约束什么

前端代码是 Natives 最频繁变更的部分，也是最容易长歪的部分。本篇约束三件事：**目录怎么分**、**组件怎么分层**、**命名怎么统一**。目标是新组件能立刻找到归属，review 时不用争论「这个该放哪」。

---

## 二、目录结构（当前生效）

```
src/
├── app/                  ← Next.js App Router 路由（页面入口，薄）
│   ├── layout.tsx          Root Layout
│   ├── page.tsx            首页
│   ├── files/              文件浏览器路由
│   ├── ai/                 AI 工作台路由
│   ├── modules/ store/ workshop/ settings/ tools/
├── components/           ← React 组件（按域分文件夹）
│   ├── shell/              外壳：Sidebar / ContentArea / RightPanel / Terminal / CommandPalette ...
│   ├── files/              文件域：FileBrowser / FilePreview / FileList / FileCard ...
│   ├── ai/                 AI 域：AgentDashboard / ChangeInbox / FollowModeUI ...
│   ├── tools/              工具域：ScreenshotCard / ReleaseWizard ...
│   ├── release/ screenshot/ onboarding/ update/ iframe/ editor/ crash/ preview/
│   └── ui/                 跨域通用 UI 原子（EmptyState 等）
├── lib/                  ← 框架层与服务层（非 UI）：theme-engine / iframe-manager / error-classifier / search-engine ...
├── hooks/                ← 可复用 React Hook（当前为空，预留）
├── i18n/                 ← 国际化：en.ts / zh.ts
├── types/                ← 全局类型定义
└── main/                 ← Main Process 模块（见 technical/）
```

#### R-E1 · 组件按业务域分文件夹，不按类型分
- **等级**：MUST
- **分类**：命名、分层
- **规则**：组件**必须**按业务域归类到 `src/components/{domain}/`（如 `files/`、`ai/`、`shell/`）。**禁止**按类型分（如 `components/buttons/`、`components/forms/`、`components/modals/`）。只有**跨域通用**的原子 UI 才放 `components/ui/`。
- **正例**：`components/files/FileBrowser.tsx`、`components/ai/AgentDashboard.tsx`。
- **反例**：把所有弹窗归到 `components/modals/` → 域归属丢失，找组件要靠记忆。
- **为什么**：按域归类让「找个功能相关代码」成本最低；按类型分在功能多时会失控。
- **检查方法**：新建组件时先问「属于哪个域」；`components/ui/` 只接收被 3+ 域复用的原子。

#### R-E2 · `app/` 路由保持薄，逻辑下沉到组件
- **等级**：SHOULD
- **分类**：分层
- **规则**：`src/app/**/page.tsx` **应该**是薄入口：取数 + 渲染对应的 `components/{domain}/` 组件。复杂逻辑、副作用、状态**应该**下沉到组件或 `lib/`。**禁止**在 page.tsx 里堆砌大段业务逻辑。
- **为什么**：路由文件易被 Next.js 约定绑架；保持薄有利于复用与测试。
- **检查方法**：page.tsx 行数若显著增长（>80 行纯逻辑），考虑下沉。

---

## 三、组件分层

#### R-E3 · 三类组件与各自的约束
- **等级**：MUST
- **分类**：分层
- **规则**：组件**必须**归入以下三类之一：

| 类 | 位置 | 特征 | 约束 |
|----|------|------|------|
| **Shell 容器组件** | `components/shell/` | 编排整体布局与全局状态（ShellLayout、Sidebar、RightPanel、Terminal） | **可以**持有全局状态、调用多个域；**禁止**被业务域组件反向 import。 |
| **Feature 业务组件** | `components/{domain}/` | 实现某个业务域的 UI 与交互（FileBrowser、AgentDashboard） | **可以**调用 `lib/` 与 `components/ui/`；**禁止** import 其它域的 Feature 组件（跨域复用下沉为 Hook 或 ui 原子）。 |
| **UI 原子** | `components/ui/` | 无业务语义、跨域复用（EmptyState、未来的 Button/Modal） | **必须**无业务依赖、可独立预览；**禁止** import 任何 Feature/Shell 组件或 `lib/` 业务模块。 |

- **为什么**：三层防止「业务组件互相 import 形成蜘蛛网」和「UI 原子被业务绑架」。
- **检查方法**：review import 方向：Shell ← Feature ← ui，不可逆转。

#### R-E4 · UI 原子必须接受主题令牌，禁止硬编码颜色
- **等级**：MUST
- **分类**：主题、命名
- **规则**：`components/ui/` 与所有组件的视觉值（颜色、圆角、间距、字号）**必须**引用 `design-tokens.ts` 的常量或 CSS 变量（`var(--xxx)`）。**禁止**硬编码十六进制色值、魔法数字尺寸。
- **正例**：`borderRadius: BORDER_RADIUS.md`、`color: 'var(--accent)'`。
- **反例**：`background: '#cdf24b'` 直接写死 → 换主题失效。
- **为什么**：主题系统（见 `ui-ux/01`）依赖令牌统一；硬编码会让三皮肤失效。
- **检查方法**：`grep -rn "#[0-9a-fA-F]\{6\}" src/components` 应只出现在 token 定义文件与受控 fallback 中。

---

## 四、命名约定

#### R-E5 · 文件与组件用 PascalCase，Hook 用 use 前缀
- **等级**：MUST
- **分类**：命名
- **规则**：
  - React 组件文件与导出的组件名**必须**用 PascalCase：`FileBrowser.tsx` 导出 `FileBrowser`。
  - Hook 文件与函数**必须**用 `use` 前缀 camelCase：`useFocusTrap.ts` 导出 `useFocusTrap`。
  - 非组件的工具模块用 camelCase：`theme-engine.ts`、`error-classifier.ts`。
  - 测试文件与被测文件同目录同名加 `.test`：`error-classifier.test.ts`。
- **为什么**：与现有实践一致（见 `src/lib/` 大量 `.test.ts` 同名并置）；统一命名降低检索成本。
- **检查方法**：新建文件时核对大小写与前后缀。

#### R-E6 · 单文件单职责，避免巨型组件
- **等级**：SHOULD
- **分类**：分层、性能
- **规则**：一个组件文件**应该**只承担一个职责。当文件超过约 300 行或含 3+ 个不相关状态时，**应该**拆分。`ShellLayout` 作为编排例外可更长，但仍**应该**把各子状态拆成自定义 Hook。
- **为什么**：大文件难读、难测、AI 编辑易出错。
- **检查方法**：review 时关注行数与职责密度。

---

## 五、本篇合规自检清单

- [ ] 我的新组件放在了正确的域文件夹，而非按类型分（R-E1）。
- [ ] 我的 page.tsx 保持薄，逻辑下沉到组件/lib（R-E2）。
- [ ] 我清楚我的组件属于 Shell / Feature / UI 原子哪一类，import 方向正确（R-E3）。
- [ ] 我没有硬编码颜色/尺寸，都引用了设计令牌（R-E4）。
- [ ] 文件命名遵循 PascalCase / use 前缀 / `.test` 后缀（R-E5）。
