# 模型代理退役后正确性修复

日期：2026-10-20。范围为缺失 usage 的投影与直连响应边界；原代理删除改动保持，未操作真实用户库、Keychain、外部工具配置、系统安装或远端 Git。

## 全局 AI 连接配置补充（20:21）

用户确认由 AI 连接统一配置 Base URL、API Key、协议与模型，供模块调用。沿用 Provider／默认引用与既有 Broker，不新增代理、schema 或第二个计费 writer。设置顶部始终显示全局默认选择器；新增连接同时填写首个模型并显式选择是否设为默认，有现存默认时不预选替换。默认不可用时显示提示，不静默替代；模型 ID 中的 `::` 保持完整。中英文文案同步，Keychain owner 不变。

| 需求 | 实现／Owner | 验收 |
| --- | --- | --- |
| 一次保存完整连接 | `providers.go` 的 `model_provider_create` 可选 `modelId`／`setAsDefault`；Repository 单次更新，Secret 仍由 Model Host 管理 | `TestCreateGlobalAIConnectionAtomically`：单次 revision、Keychain 引用、options 默认可解析；新增非默认连接保留原默认；旧调用用例保持 |
| 失败不留半成品 | 校验在 Keychain 写入前；版本／持久化失败沿用既有回收 | `TestGlobalConnectionSetupFailuresPreserveState`：缺失／空白／超长 ID、默认连接禁用、revision 冲突、Keychain 不可用均保留原默认、连接与密钥 |
| 全局入口及可用反馈 | `model-settings-view.js`、控制器、CSS 与双语 locale | `rtk node scripts/model-connection-browser.test.mjs`：真实 Chrome 加载生产 view/controller 和 Files 基础样式，使用隔离 Native API fixture；Archive／Volt × 中英文 × 1280／360 px 共 8 组通过，键盘提交、失败保留草稿、显式默认／清除／切换、已有默认不替换、失效提示、无 workspace 横向溢出；检查窄窗截图 |

`rtk npm run model:host:check`：190 项／13 packages 通过；Go 修改文件格式检查无输出。`rtk npm run perf:check` 本次退出 0，覆盖 Extension、Files、Apps、架构边界、安全、端口与性能门禁。生产调用链、Tokenusage owner 与 Rust／模块 UI 本轮未变，不重复此前通过的相关检查。浏览器 fixture 首次因两个 refresh 按钮造成 locator 歧义失败，已改为准确的 `data-role="refresh"`；补齐 Files 基础样式后重新核验截图及 8 组交互，不修改生产 CSS 规避夹具问题。

本轮只交付源码与验证，不更新真实安装、不改用户凭据／默认配置、不调用真实供应商。本轮浏览器证据是页面与控制器检查，不能当作完整 Chrome Native／真实供应商／双入口或发布验收；此前失败及限制仍保留如下。

## 根因与修复

1. Recorder 已区分 `usage_status=missing`，但汇总没有携带覆盖情况，零费用补算排除零 Token，导致缺失 usage 被标记为 `priced=true`。现由同一 Usage Store 在筛选后的汇总、趋势、各排名及会话返回缺失请求数；已知 Token／金额保留，无数据、报告的真实零值、缺 usage、缺单价及混合覆盖分别处理。Go → Rust DTO/API → Tokenusage、空间统计、设置及顶栏消费同一语义；未新建数据 writer 或修改 schema。
2. Responses 没有文本 delta 时从终态 response 提取正文，绕过增量分支的长度上限。现同时检查终态正文及流式返回边界，非完成状态不能凭正文冒充成功；保留 usage，继续受现行传输、超时及 owner 上限约束。
3. 顶栏只读数据库回退兼容尚无 `usage_status` 的旧库，不执行迁移；HTTP 映射、周期摘要和面板保留缺失数，今日缺失记录不会被当作无活动而切换到累计历史。

## 复现与回归

| 对象 | 修复前 | 修复后 |
| --- | --- | --- |
| `TestMissingUsageDoesNotBecomeFreeConsumption` | 缺失 usage 被标成完整零费用，失败 | 通过 |
| `TestResponsesTerminalOutputLimit/8001` | 8001 个中文字符返回成功，失败 | 超限明确失败；usage 仍为 10／20／30；8000 边界通过 |
| `TestUsageCoverageAcrossAggregates` | 新增针对性回归 | 真实零值、缺失、已知＋缺失混合及过滤通过；模型／供应商／来源／访问密钥／小时／会话保留缺失数与已知小计 |
| `TestResponsesIncompleteBodyIsNotSuccess` | 新增针对性回归 | 流式／非流式未完成、失败状态不报告成功，计量证据保留 |

## 已完成检查

- `rtk npm run model:host:check`：182 项／13 packages 通过，使用隔离配置根。
- `rtk env -u CARGO_TARGET_DIR cargo test --workspace`：退出 0，含现有 50 次生命周期循环。原有 unused import／dead code 警告及历史 ignored 测试保持，不宣称零警告。
- `cargo fmt --check`、模块 UI build/check、首次 `perf:check`（包含 Extension、Apps、边界、安全、端口与性能门禁）通过；Tokenusage 编译期资源仍来自 `ui/dist`。随后仅补顶栏趋势覆盖标记，Objective-C 编译及 Chrome 数据／面板测试复验通过；其余源码不变，复用首次门禁结果，重复门禁异常另列如下。
- 定向 `go test -race ./internal/usage -run 'Test(MissingUsageDoesNotBecomeFreeConsumption|UsageCoverageAcrossAggregates|Panel)' -count=1 -timeout=60s`：10 项通过；host 与 inference 的全包 race 同次退出通过。
- 顶栏 Objective-C 语法检查、`scripts/statusbar-browser.test.mjs` 的原生数据自检及真实 Chrome 面板通过，覆盖旧库、缺失覆盖、未知今日不回退、部分 Token／费用及键盘切换。
- Tokenusage 生产 dist 在真实 Chrome 覆盖 Archive／Volt × 中英文 × 1280／320 px 共 8 组：未知为 `--`，部分为 `10+`／`≥$0.25`，报告零值为 `0`／`$0.00`，缺失说明与模型切换键盘操作通过，页面错误为 0。数据为隔离 HTTP fixture，不能替代完整 Native 或真实供应商。
- 单独真实 Native Messaging 链通过：当前源码编译的进程测试快照、内存测试 Secret、隔离 HOME/config、可控 SSE 供应商 → 成功正文 → 恰好 1 条 missing 用量事件 → Native overview 返回 `missingUsageRequests=1`、`priced=false` → events 返回 `usageStatus=missing`／`costStatus=unpriced`。该链不冒充 Chrome Shell、完整产品或真实供应商。

## 原始失败与未验范围

- 重复 `perf:check` 在既有 `space-widgets-structural.test.mjs` 输出全部断言通过后未退出。观察到该 Node 进程持有外部 HTTPS 连接，进程采样停在 `uv__io_poll/kevent`；只终止本次运行的精确子进程，命令退出 143。未改测试、阻断网络或把此次中止记为通过；其与本次改动的关系未独立定位。该脚本及其调用参数未改动，首次同一实现的完整门禁通过，后续顶栏改动由独立编译／Chrome 检查覆盖；不重复循环直到变绿。
- 全包 `go test -race ./internal/usage ./internal/host ./internal/inference -timeout=120s` 中 usage 包在 `TestScaleProbe` 超时。夹具为 **10 万**事件，栈停于 SQLite 的会话查询排序，未见锁等待；未放宽超时、删除用例或宣布性能根因已解决。普通全量测试与定向 race 通过不能替代该失败，是否存在性能回归待独立比较。
- 首次 Tokenusage UI 检查脚本把 visibilitychange 发给 window，实际监听在 document，未触发刷新而等待超时。改用生产刷新按钮后 8 组通过，未为测试改写生产刷新逻辑。
- 隔离签名候选的 Chrome 扩展路径在 `page.goto` 前未成功加载：`ERR_BLOCKED_BY_CLIENT`；独立检查扩展管理页为无扩展、无 worker。未改变 Chrome 安全策略，未机械重跑完整链；本轮扩展 Shell 与本机 Web 的完整浏览器直连链仍未验证。
- 真实供应商、系统安装／签名／公证／发布、多平台及人工读屏未验证；原整体迁移状态保持。此次未改变常驻／连接 owner，也未新增后台工作，未运行 30 分钟长测，不以本轮短测替代已有长测缺口。

临时诊断脚本和 JSON 位于本次任务的 `.tmp/model-proxy-closeout/`；长期结论与未验边界以本记录为准，不依赖临时目录继续存在。
