# 第三方项目引用说明

> 本文只记录仍被当前生产代码使用或作为行为参考的外部项目。它不是架构规范；产品边界以
> `docs/README.md`、`docs/standards/`、适用 ADR 和 `docs/contracts/` 为准。

## 已纳入构建的代码

当前 `model-host` 不链接任何模型代理／路由内核。ADR-0044 已删除
`third_party/cliproxyapi` 及其 SDK、补丁和构建依赖；内部 AI 调用由 Model Host
按供应商协议直接发起。

## 本地行为参考（未复制）

以下目录不属于本仓库产物，不得作为运行时依赖：

| 项目 | 用途 | 当前落点 |
|---|---|---|
| EasyCLIProxyAPI | 历史用量导入格式（V3 `usage.db`）与价格源的兼容参考；不参与推理 | `model-host/internal/usage/importer.go`、`pricing.go` |
| TablissNG | 空间 Widget、背景和主题行为参考 | `extension/space*`、`extension/plugins/` |
| FundVal-Live | 基金业务规则、来源和 provenance 参考 | 内置 Fund 模块的产品实现与测试 |

参考项目的桌面壳、独立模块分发、在线目录、管理 API、数据库和凭证布局均不进入 Natives。

## 浏览器侧公共服务

扩展只请求 `extension/manifest.json` 明确声明的域名；Native Host 不联网。新增域名必须同时更新
manifest、隐私说明、失败状态和对应测试。

| 域名 | 用途 | 使用文件 |
|---|---|---|
| `api.open-meteo.com`、`geocoding-api.open-meteo.com` | 天气与地理编码 | `extension/plugins/widgets/weather.js` |
| `open.er-api.com`、`api.coingecko.com` | 汇率与行情 | `extension/plugins/widgets/currency-rates.js` |
| `api.ipify.org` | IP 信息 | `extension/plugins/widgets/ip-info.js` |
| `github-contributions-api.jogruber.de` | GitHub 贡献图 | `extension/plugins/widgets/github.js` |
| `api.unsplash.com`、`images.unsplash.com` | 背景图片 | `extension/plugins/backgrounds/unsplash.js` |
| `api.nasa.gov`、`apod.nasa.gov` | APOD 背景 | `extension/plugins/backgrounds/apod.js` |
| `api.wikimedia.org`、`upload.wikimedia.org` | Wikimedia 背景 | `extension/plugins/backgrounds/wikimedia.js` |
| `api.giphy.com` | Giphy 背景 | `extension/plugins/backgrounds/giphy.js` |
| `www.google.com/s2/favicons` | 网站图标 | `extension/plugins/widgets/links.js`、`bookmarks.js` |
| `duckduckgo.com`、`en.wikipedia.org`、`www.google.com` | 搜索建议 | `extension/plugins/widgets/search.js` |

测试使用离线 mock；不得把公共服务可用性当作本地产品就绪条件。

## 仓库范围

发布包只包含当前构建所需的 `extension/`、`crates/`、`model-host/`、`third_party/`、`installers/`
和 `scripts/` 文件。`node_modules/`、`target/`、`dist/`、数据库、环境变量和凭证不入库、不进用户包。
历史架构与分发方案位于 `docs/archive/`，不参与构建或门禁。
