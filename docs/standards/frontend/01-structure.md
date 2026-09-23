# 前端 01 · Extension 结构与模块边界

> 版本：4.0.0 · 日期：2026-09-14
> 当前源码：`extension/`

## 当前目录角色

```text
extension/
├─ *.html                    页面入口，静态且薄
├─ newtab.js / files.js /
│  apps.js / app.js          页面 controller 与装配
├─ *-client.js / *-api.js    Native Messaging 领域 client
├─ files-*.js                Files 领域模块
├─ space-*.js                Workspace/Grid/Canvas/设置模块
├─ ai-performance/           AI 效能查询、指标与 renderer
├─ plugins/widgets/          构建期内置 Widget definitions
├─ plugins/backgrounds/      构建期内置背景 definitions
├─ _locales/zh_CN|en/        Chrome i18n
└─ *.test.mjs / tests/       定向与真实浏览器测试
```

目录名 `plugins/` 是现有构建期定义集合，不代表 Plugin Runtime；其中代码随 Extension 发布，
不得在运行时下载或执行第三方代码。

#### R-E1 · HTML 入口保持薄

- **等级**：MUST
- HTML 只声明语义结构、固定资源、CSP 允许的模块入口和无脚本基础状态。
- 业务查询、生命周期和状态机进入页面 controller 或领域模块。
- 禁止 inline script、远程 script、动态代码和页面内复制大段业务逻辑。

#### R-E2 · 文件按领域和职责组织

- **等级**：MUST
- Files、Space、Apps、Model、AI Performance 各自在现有前缀/目录下维护。
- 页面 controller 负责装配；领域模块负责一个明确能力；纯 helper 不访问 DOM/Host。
- 跨域共享前先确认至少两个真实调用方，优先复用已有模块。

#### R-E3 · import 方向固定

- **等级**：MUST

```text
page controller → domain controller/renderer → domain client/pure helper
```

- client 不 import 页面或 DOM；renderer 不直连 Chrome Native Messaging。
- Widget/background definition 不反向 import `space-dashboard.js`。
- 模块业务代码不进入 Extension；`app.js` 保持通用。

#### R-E4 · Native 通信只经领域 client

- **等级**：MUST
- 原始 `chrome.runtime.connectNative` 只出现在统一 Native client 或受审计测试入口。
- 页面使用 Files/Model/App 领域 client；method、timeout、pending 和 disconnect 由 client 管理。
- 不在 UI handler 中拼装第二套帧协议。

#### R-E5 · 样式按 Surface 归属

- **等级**：MUST
- 页面级 CSS 与对应 Surface 同目录；共享值使用现有 CSS custom properties。
- 空间内 Widget 必须消费 `--space-widget-*` 局部角色，不能回读 Files/Host 页面颜色。
- CSS fallback 只保证可读性，不建立第二主题。

#### R-E6 · 命名遵循现有仓库

- **等级**：MUST
- 文件使用 kebab-case；函数/变量 camelCase；常量 UPPER_SNAKE_CASE；class 名使用可读语义。
- 测试与被测域同位或进入 `extension/tests/`；fixture 明确标注非生产。

#### R-E7 · 文件规模按职责拆分

- **等级**：SHOULD
- 页面 controller 只装配；超过约 700 行时检查是否存在可独立 owner、生命周期或状态机。
- 不为满足行数创建 `part1`、`utils`、`helpers`；拆出的模块必须一句话说清职责。

## 合规自检

- [ ] HTML 薄且无 inline/远程代码。
- [ ] import 方向没有反转。
- [ ] Native 调用经过领域 client。
- [ ] 模块业务未进入 Extension。
- [ ] 空间组件使用空间局部样式角色。
