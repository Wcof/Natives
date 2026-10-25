# 网页空闲恢复与断线文案修复

依据：[ADR-0047](../../adr/0047-web-document-connection-and-idle-resume.md)。仅改 Web 连接与通用模块 Shell，不修改真实持仓、模型凭据或计费逻辑。

## 根因与实现

- 页面隐藏释放 Surface 和 SSE 后，Core 无 owner 而退出；旧普通网页没有 Native 启动权限。现有每页共享事件适配器在首次授权连接后持有 document owner，隐藏改用无业务 Surface 的被动 SSE，停止业务事件分发与后台重连轮询。模块、Files 与 Model 业务租约仍按原规则释放；pagehide 关闭连接，pageshow 恢复一次。
- 返回可见后重新创建 document Surface；通用 Shell 遇到 Worker 被回收、新端口替代旧 iframe 时，自动重建 sandbox，继续现有握手／启动流程，不重放写入。
- SSE 释放时记录实际最后活动时刻，最后 owner 离开后重新计算 Core 原有 60 秒宽限。
- 语言资源获取失败时原适配器直接返回 key。断线所需三条文案现从现有 zh_CN／en locale 单源生成至 `extension/platform/connection-messages.js`，随模块加载并编入 Core；语言请求 2 秒有界，失败、超时、缺 key 都有本地化回退。无第二份手工文案。

主要实现：`extension/platform/events.js`、`surface.js`、`i18n.js`、`extension/app.js`；资源生成：`scripts/modules/build-ui.mjs`；Core：`service/events.rs`、`static_files.rs`。连接仍受 Cookie／Origin 和原有容量约束，扩展与 Service Worker 没有新增保活。

## 回归证据

- 红例：`rtk proxy node extension/platform/recovery.test.mjs` 在修复前失败，actual=`webConnectionRequiredTitle`，expected=`请从 Natives 打开此页面`。
- 修复后该用例覆盖语言请求失败／缺 key／超时、唯一 dormant SSE、不续租业务、隐藏不分发（包含断线失效事件）、业务 client 释放后 document 仍可恢复、pagehide／BFCache 与新 Surface。
- Rust `service::events` 两项通过：跨会话任务进度仍拒绝；document 无业务 Surface 时只持有流，流关闭清空登记并重新起算 Core 宽限。
- `scripts/web-idle-recovery-browser.test.mjs` 用当前签名产品源、隔离 HOME／工作区／应用库、真实 Core 与真实 Fund，普通 Chrome CDP `noDefaults` 保留真实可见性。8 组 Archive／Volt × 中文／英文 × 1280／320 在语言请求失败时连接文案可读，键盘重试及无横向溢出通过；窄窗截图已查看。空 HTML 夹具仅用于连接提示；进程回收和恢复使用真正的 `app.html?app=fund` 与 Fund iframe。
- 两轮真实生命周期均观察到隐藏约 141 秒后 Fund Worker=0、业务 Surface=0，Core 仍可恢复；返回页面自动创建新 PID 的 Fund，未出现连接提示；关闭最后页面约 62 秒后 Core 正常退出，页面错误数组为空。最终连接 owner 已从页面 controller 收回平台适配器，消除首轮边界门禁发现的循环依赖。第二轮原始记录：`.tmp/web-idle-recovery/run-1x6skZ/report.json`，日志：`.tmp/web-idle-recovery/browser-final.log`。
- 既有 `scripts/web-no-pairing-browser.test.mjs` 的 8 组真实 Chrome／Core 检查通过，保留 Bootstrap、Cookie、刷新、注销、无配对及限速边界。

## 门禁及范围

Rust workspace：27 suites／618 passed／0 failed／13 ignored；最终 Native package 110 passed。Rust fmt、模块 UI 构建／检查、最终 `perf:check`（含 Extension／Files／Apps／架构／鉴权／端口）通过。首轮循环依赖失败已保留并修复；测试夹具首次因隔离 HOME 未携带 Rustup 路径失败，修正 fixture 环境，没有修改系统 toolchain；Node 纯模拟 fetch 的超时测试补足测试进程存活句柄，没有改变生产超时。

Release Core 的前后相同隔离数据、单个已授权无业务 SSE 对照见 `.tmp/web-idle-recovery/resources-dtfr32s6/`；每版 60 秒／5 个窗口（12 秒）采样，before RSS max=6112 KiB，after RSS max=6064 KiB、CPU max=0.0832%。后续交付复验的 Files p95=51.802416 ms＞50 ms，定向复验又出现 isolated service failed to start；独立当前 Release 启动探针随后 HTTP 200 且 stderr 正常。两次失败保留，不无证据归因为本次修改或环境；最终串行 perf:check 退出 0，Files p95=45.126375 ms，未改预算或 Files 实现；原始日志 .tmp/web-idle-recovery/perf-check-serial.log。只报告观察范围，不把 debug 进程证据当 Release 性能，也不宣称性能提升。

没有新增守护进程、登录启动、端口、免鉴权启动 API 或持久浏览器 Secret。显式停止／崩溃后的 Core 仍由原受信 Natives Launcher 启动，普通网页不能自行执行本机程序。完整 30 分钟资源长测、Windows／Intel、安装及发布未执行；本次新增 document 连接已用真实 141 秒回收／恢复与 Release 对照覆盖，长期资源结论仍未整体验收。

## 当前本地入口

2026-10-21 10:18：通过现有 --launcher-web 打开入口；首次短命工具命令返回 opened_web 后服务未保持连接，未将其当成功。随后使用当前开发 Core 的前台受控会话启动并沿同一 Launcher 交接，8771 的 healthz=200、活动 Surface=2；platform/events.js、platform/i18n.js、platform/connection-messages.js 和 app.js 与当前修复源码逐字节一致。未注册系统服务／修改安装配置或真实业务记录。用户正在使用其他 Chrome 页面，因此未进一步操作其窗口或把 API 状态当人工页面验收；旧标签页可能需要刷新一次以加载新脚本。
