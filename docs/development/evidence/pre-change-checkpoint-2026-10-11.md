# 投资改版前代码检查点

时间：2026-10-11 13:58（北京时间）。用途：按用户明确指令，保存并推送当前项目代码，供后续投资产品整改从同一基线开始。不是发布或部署验收。

## 范围

- 分支：`deploy`；远端：`origin/deploy`。提交前 HEAD 为 `175fa1649387`，远端为 `0a6d46584f92`；已 fetch 并确认远端落后本地 2 个提交，无需改写历史。
- 初始候选为 395 个文件，包含投资账本／卡片／动态收益／自选移动、Archive／Volt 主题、模块边界、Host 生命周期和开发重启，以及对应测试／工程文档。
- `.tmp/`、顶层 `dist/`、`target/`、数据库及个人配置遵守现行 ignore。模块 `ui/dist` 是版本内编译资源，按现行构建生成并提交。
- 文件名与常见密钥模式扫描未发现真实凭据或用户投资库；仅命中测试中的 `user:pass` URL 夹具。该扫描不是完整安全审计。
- 本次补齐 README 的持仓卡片／分类／四类收益描述，并记录验证事实；没有实施独立原型中的新组合设置、水位背景、原地 AI 或基金季度详情。
- 外部 `/Users/ldh/Downloads/project/plan/memory/` 和 `/Users/ldh/Downloads/project/demo/natives/` 不属于本 Git 仓库，不随本次 push 保存；项目内导航引用保留。

## 本轮验证

验证结束时间：2026-10-11 14:01。

| 对象／命令 | 结果 |
|---|---|
| Rust fmt --check | 通过 |
| Rust workspace --no-run | 编译通过 |
| Rust workspace | 通过，Fund 342 项／2 项既有忽略；50 次 Runtime 生命周期循环通过 |
| model:host:check | 通过，210 项／14 个包 |
| 修改的 Go 文件 gofmt -l | 通过，无未格式化文件 |
| modules:ui:build | 通过，生成资源与检查前摘要一致 |
| modules:ui:check | 通过，资源检查及 4 项主题／6 项绘图测试 |
| investment-suite-browser.test.mjs | 通过；合成 API 的资产／流水／编辑失败、取消、隐私、双主题／双语／三宽路径 |
| home-tab-retention-browser.test.mjs | 通过；真实 Chrome hidden→visible、同快照／DOM／草稿、手动刷新；API 为隔离夹具 |
| NativesStatusBar.m clang -fsyntax-only | 通过 |
| perf:check | 通过，包含 extension:check／apps:check；10k 目录 p95 39.40 ms，Model Host 本探针快照 829.18 ms／EOF 72.82 ms／稳态 CPU 0% |
| standards:check／git diff --check | 通过；暂存内容另执行 git diff --cached --check |

Rust 编译保留 4 条既有测试警告：`Collected`、`Write` 未使用导入，`sell` 未使用函数及 `status` 未使用变量。本次保存基线没有顺手修改无关测试。

## 验证限制

Native 文件性能探针的 portable CPU／GPU 项为 unsupported，Model Host 探针使用 Debug；这些数据不是长期 Release 性能提升证据。

本轮没有重新执行全部生产 Chrome→Native 投资用户流程、真实供应商、真实用户账本、30 分钟资源长测或跨平台安装验收。此前证据和未完成事项保留在 ROADMAP；不能把本次代码检查点称为整体产品或正式发布完成。推送仅触发当前性能 CI，不包含部署步骤。
