# 第三方项目引用与架构设计说明文档 (THIRD_PARTY_REFERENCES)

> 本文档详细记录 Natives 项目在架构设计、模块实现及组件迁移过程中所引用的外部项目、参考源码及第三方服务，便于后续开发与 AI 交互理解。

---

## 一、核心第三方引用与架构集成清单

### 1. EasyCLIProxyAPI
- **参考位置**：`/Volumes/UNTITLED/本人材料/project/EasyCLIProxyAPI`
- **定位与作用**：基于 CLIProxyAPI 构建的前置桌面 GUI 代理参考实现。
- **引用与对齐内容**：
  1. **使用记录明细字段对齐**：
     - 文件：`extension/model-usage-view.js`、`model-host/internal/usage/`
     - 对齐字段：时间（Time）、模型（Model）、输入 Token（Input）、输出 Token（Output）、缓存 Token（Cache）、缓存率（Cache Rate）、总计 Token（Total）、生成速度（Speed: tokens/s）、首字延迟（TTFT: ms）、总耗时（Latency: ms/s）、费用预估（Cost）、状态（Status）。
  2. **智能体配置（Agent Clients）生命周期与交互设计**：
     - 文件：`extension/model-agent-*.js`、`model-host/internal/agentclients/`
     - 对齐模式：11 类本机智能体客户端（Claude Code、Codex、Pi、OpenCode、Hermes 等）探测、配置修改写入、Claude 角色映射与模型选择器联动。
  3. **认证文件与账号模型管理（Auth Files & Account Models）**：
     - 文件：`model-host/internal/authfiles/`、`extension/model-auth-files-view.js`、`extension/model-account-models-dialog.js`
     - 对齐模式：支持纯凭据文件的模型可用性设置与排除模型持久化回写。

---

### 2. CLIProxyAPI (v7 Kernel)
- **源码与依赖路径**：
  - 本地参考路径：`/Volumes/UNTITLED/本人材料/project/CLIProxyAPI`
  - 源码内嵌路径：`third_party/cliproxyapi/`（Go 模块通过 `replace` 指令集成）
  - 版本标记：`third_party/cliproxyapi/version.txt`（当前锁定版本：`v7.2.152`）
- **定位与作用**：多供应商 OAuth 聚合网关内核。
- **引用与对齐内容**：
  1. **网关核心运行时**：
     - 文件：`model-host/internal/cliproxy/runtime.go`、`model-host/internal/host/gateway_runtime.go`
     - 作用：统一调度 OpenAI、Claude、Gemini、Antigravity、Kimi、xAI、Vertex 等多渠道 OAuth 凭据轮询与模型转发。
  2. **内核热检查与一键更新**：
     - 文件：`model-host/internal/host/kernel_handlers.go`、`extension/model-gateway-view.js`
     - 作用：支持检测上游 CLIProxyAPI 最新 git tag 版本，并一键同步 `internal/` 与 `sdk/`，自动重载 Gateway 运行时。
  3. **静态模型注册表桥接**：
     - 文件：`third_party/cliproxyapi/sdk/cliproxy/static_models.go`
     - 作用：保持各渠道 OAuth 静态模型定义与动态检索接口的无缝兼容。

---

### 3. TablissNG (Chromium 空间组件基线)
- **定位与作用**：个人空间（Space / Dashboard）的 24 种组件与 9 种动态背景的设计与交互规范。
- **引用与对齐内容**：
  1. **组件实现与样式层级**：
     - 文件路径：`extension/plugins/widgets/`（共 24 个组件）与 `extension/plugins/backgrounds/`（共 9 种背景源）。
     - 遵循 TablissNG 专属样式（Sass/CSS 架构），保持与 Dashboard 画布的无干扰解耦。
  2. **个体 AI 效能组件增强**：
     - `widget/todo`：番茄工作法（Pomodoro）专注计时、P0/P1/P2 优先级循环、Markdown AI 规划任务清单一键批量导入、Eisenhower 四象限 Prompt 生成。
     - `widget/notes`：实时字数与阅读耗时统计、Prompt 框架/架构方案设计/Bug 排查/代码审查多场景 AI 模板、轻量安全 Markdown 呈现。
     - `widget/search`：聚合 ChatGPT、Claude、DeepSeek、Perplexity、Devv AI、豆包、秘塔等主流 AI 引擎，提供「原理剖析/代码实现/方案对比/故障排查」一键追加修饰标签。
     - `widget/links`：国内/海外 AI 导航预设一键导入，基于首字母徽章的 Favicon 加载失败降级容灾。

---

### 4. Open-Meteo API & 汇率开源服务
- **定位与作用**：纯前端无 Key、免鉴权的轻量天气与汇率数据源。
- **引用与对齐内容**：
  1. **天气服务 (`widget/weather`)**：
     - API 端点：`https://api.open-meteo.com/v1/forecast`
     - 级联省市数据：`extension/plugins/widgets/weather-cities.js`
     - 交互隔离：空间正文仅呈现天气概览、温度与预报，所有地区选择器集中于卡片抽屉配置（`renderSettings`）内。
  2. **汇率服务 (`widget/currencyRates`)**：
     - API 端点：`https://open.er-api.com/v6/latest/`
     - 支持常用法定货币与加密资产实时汇率转换。

---

## 二、架构隔离与安全守则 (ADR-0020)

1. **凭据安全与零泄露**：
   - 所有运行时 OAuth Access Token、Secret 与 API 密钥均由 OS Keychain（macOS Keychain / Linux Secret Service）统一接管，持久化数据不存储明文凭据。
   - Git 仓库严格忽略所有 `.token`、`.key`、`*auth_file*.json`、`*oauth*.json` 文件。
2. **纯原生 Chrome MV3 架构**：
   - 移除所有历史 V1/V2 遗留框架（Tauri / React / WebView / iframe），全站采用 Chrome Native Messaging + 纯原生 ES Modules。
   - 严格遵循 CSP 规范，杜绝任何内联脚本。
3. **包体积预算与性能守护**：
   - Extension Bundle 严格守卫在 **300 KiB**（307,200 bytes）预算之内。
