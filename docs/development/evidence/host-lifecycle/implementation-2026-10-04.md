# Host 进程生命周期整改 · 实施与验证记录（2026-10-04）

实施对象：`docs/architecture/application-performance-remediation.md`「进程生命周期整改与交接」章节。
实施范围：T0～T5 全部业务修复 + T6 本地可执行验证；保留其他 Agent 的全部未提交改动。
执行环境：macOS arm64（darwin 25.6.0），go1.26.6，仓库唯一 `target/`，全部命令经 rtk。

## 修改文件与模块职责

### Model Host（Go）

| 文件 | 职责与改动 |
|---|---|
| `internal/lifecycle/lifecycle.go`（新增） | 最小生命周期原语：`Registry`（任务登记/stopping 原子判定/取消/共同 deadline 等待）、`Reap`（子进程异步 Wait 恰一次）、`Shutdown`（单一绝对 deadline）。仅标准库，无泛化任务框架 |
| `internal/lifecycle/lifecycle_test.go`（新增） | 登记/停止/超时/并发竞态/reap 语义 7 项测试 |
| `clients.go`（新增） | clientHub 重构：admission ≤32、每客户端有界输出（64 槽响应队列 + 2 MiB 字节上限）、250ms 入队期限、事件按类型合并、一致清理顺序（移出 hub → 关连接 → 取消该客户端 owner 任务 → idle 信号）、closeAll 幂等且有界等待 |
| `main.go` | accept goroutine 退出联动（未交付连接当场 Close）；退出顺序统一为 closeAll → engine.Close → instance.Close |
| `internal/host/engine.go` | `closed` 原子 stopping 门（Close 后 Handle 拒绝新请求）；Close 按 L4 顺序共享 800ms 绝对 deadline；采集任务入 Registry；`CancelInferenceForOwners`（客户端断开只取消其 owner 任务）；stopRuntime 持锁时间受 ctx 期限约束 |
| `internal/host/inference.go` | `InferenceOwner`（与 parseInference 同源的 owner 提取） |
| `internal/usage/plugin.go` | `Close(ctx)`：有期限排空，超时显式报告未持久化记录数（不静默丢弃、不无限等待） |
| `internal/usage/collector*.go`、`grok_session.go` | ctx 贯穿遍历/文件批读/取消披露（cancelled 显式状态）；遍历节点 ≤200k、单目录扇出 ≤20k、错误列表 ≤200（超出以 ErrorsTruncated 计数披露）；已提交游标保持完整（重扫不漏计不重复计费） |
| `internal/nativeio/nativeio.go` | `MarshalResponse`/`WriteEncoded`：预编码响应供队列字节核算，避免双重序列化 |
| `internal/agentclients/launch.go`、`internal/authfiles/manager.go` | F3：全部启动器 spawn（open/osascript/cmd/terminal/xdg-open）Start 后 `lifecycle.Reap` 异步回收，消灭僵尸路径；不终止用户打开的终端/外部应用 |
| `internal/singleinstance/instance_windows.go` | F7：管道名由「用户身份 + 规范化配置命名空间」派生（SHA-256 前 12 字节）；旧固定管道不静默连接，返回可操作重启提示；ACL 保持属主完全控制 |
| `cmd/export-usage-fixture/main.go` | CollectSources 签名跟随（ctx） |

### SDK（third_party/cliproxyapi，F1）

| 文件 | 改动 |
|---|---|
| `sdk/cliproxy/usage/manager.go` | 容量真实生效（buffer ≤0 回落 512）；总字节 ≤8 MiB；入队记录不再持有请求 ctx（dispatch 用 detached ctx，下游断连不丢已完成用量）；诊断 body 截断 4 KiB、响应头快照 ≤8 KiB（tokens/费用/身份计费原子不动）；满队列可取消背压；出队清空槽位引用；Stop 排空 + 有界 join（5s）；Stop 后 Publish 同步派发不复活 dispatcher；dispatcher 每次创建独立 exited channel（修 double-close 缺陷） |
| `sdk/cliproxy/usage/manager_lifecycle_test.go`（新增） | L08/L09：容量边界/背压解除/排空 join/不复活/字节截断计费字段不动/50 次启停基线/producer-Stop 竞态 10 轮 |
| `natives-patches/0001-natives-fork-customizations.patch` | manager.go 段整体重新生成；对新鲜上游 v7.3.17 检出 `git apply` 重放后与 fork 文件逐字节一致（APPLY_OK + MANAGER_IDENTICAL） |
| `NATIVES_FORK.md` | 补丁描述同步为有界队列语义 |

### Files Host / App Runtime（Rust，F6）

| 文件 | 改动 |
|---|---|
| `native-file-host/src/main.rs` | locate 从同步 dispatch 移入有界任务路径（受 MAX_CONCURRENT_JOBS 约束、可取消）；`locate_cancel` 加入取消白名单 |
| `native-file-host/src/search.rs` | `run_locate(request, cancelled)`：取消 token 贯穿 walkdir 与 mdfind 回退；`stream_bounded_mdfind` 流式限额（64 KiB/2000 行）+ 2s 期限 + kill 后 wait/reap + 读线程 join；`pdf_text_match` 非 kill 路径 wait 加 2s 有界轮询（kill 兜底） |
| `native-file-host/src/dispatch.rs`、`tests.rs` | 签名跟随 |
| `app-runtime/src/controller/stopping.rs` | 看门狗去重（进程内仅 spawn 一次）；触发时记录最近阶段（watchdog_timeout phase=…，强制退出不记作优雅完成）；accept 阻塞解除（向自身回环端口发唤醒连接） |
| `app-runtime/src/http/mod.rs` | `HttpSlotGuard` RAII：请求线程 panic 也回落并发计数；shutdown 后迟到连接直接丢弃 |
| `app-runtime/src/http/client.rs` | 写超时 30s（慢客户端不长期占用请求线程） |

### 顶栏与 dev launcher

| 文件 | 改动 |
|---|---|
| `installers/macos/resources/NativesStatusBar.m` | ensureModelHostAlive 两处就绪等待加 0.1s 步进（原来毫秒级烧完尝试）；新增 `shutdownModelHostConnection`（退出时关闭自有 stdin 写端与 drain handler，不终止其他客户端）；actQuit 调用 |
| `extension/dev.mjs` | F5：废除按 debug/release 路径清扫全部实例；改为只清理本会话登记且身份核验（启动时间/命令行）的子进程，优雅期 2s 有界等待；构建前冲突检测（待替换二进制被其他会话/Chrome 占用时报告并阻止，不覆盖使用中产物）；PID 复用防护；自测夹具同步 lstart 格式 |
| `.gitattributes`（新增） | 仅对 `natives-patches/*.patch` 关闭 whitespace 检查（内嵌 diff 上下文行的格式固有误报；HEAD 既有版本同样触发，已验证）；不影响源码检查 |

## F1～F8 → 修复对应

| 编号 | 修复位置 | 状态 |
|---|---|---|
| F1 SDK 队列无界 | manager.go 有界化 + patch 重放 | 完成 |
| F2 Hub/Writer 阻塞链 | clients.go 有界队列/合并/期限 + plugin Close(ctx) | 完成 |
| F3 spawn 无 reap | lifecycle.Reap 全部启动器接入 | 完成 |
| F4 任务取消/等待缺失 | Engine Registry + closed 门 + owner 取消 + accept 联动 | 完成 |
| F5 dev 清扫全部实例 | dev.mjs 会话隔离 + 冲突检测 + PID 核验 | 完成 |
| F6 Files/App 阻塞退出 | locate 有界化、mdfind/pdftotext 有界 reap、accept 唤醒、RAII 计数、看门狗去重 | 完成 |
| F7 Windows 管道无隔离 | 用户+命名空间派生管道名、拒绝旧管道静默兼容 | 代码完成；实机验证未做（blocked） |
| F8 采集无 ctx/预算 | ctx 贯穿 + 节点/扇出/错误上限 + ErrorsTruncated | 完成 |

## 验证证据（本地 macOS arm64）

| 检查 | 结果 |
|---|---|
| `rtk npm run model:host:check` | 203 passed / 14 packages（基线 186 → 新增 17 项生命周期测试） |
| `rtk go test -race`（lifecycle/usage/main） | 通过，无数据竞争 |
| SDK `rtk go test ./sdk/cliproxy/usage/` | 34 passed（含 5 项新 L08/L09 测试） |
| SDK race | 通过 |
| fork patch 重放 | 新鲜上游 v7.3.17 `git apply` 成功，manager.go 与 fork 逐字节一致 |
| `rtk env -u CARGO_TARGET_DIR cargo fmt --all -- --check` | 通过 |
| `rtk env -u CARGO_TARGET_DIR cargo test --workspace` | 25 套件全部 ok / 0 FAILED（含 app-runtime lifecycle 7 项、lifecycle_deadline 3 项、fund_integration、native-file-host 75 项） |
| 顶栏 clang 编译 | 通过（修改后的 NativesStatusBar.m + launcher-main.m） |
| `extension/dev.mjs --self-test` | 通过 |
| Windows 交叉编译 `GOOS=windows go build ./internal/singleinstance/` | 通过（仅语法/类型验证） |
| `git diff --check` | 通过（patch 文件 whitespace 例外经 .gitattributes 显式登记，理由见上） |
| `rtk npm run perf:check` | exit 0（目录 p95 46.07 ms ≤ 50 ms；架构检查零 ≥1000 行、禁用 API 清洁；Extension/Apps/standards 同链通过） |

## L10 退出期限实测（隔离 HOME/configDir/Keychain namespace，debug 构建）

| Host | 样本 | 中位数 | 最大 | 契约 |
|---|---|---|---|---|
| Model worker stdin EOF | 5 | 3.9 ms | 400 ms | ≤1 s ✅（样本 `.tmp/host-lifecycle/model-eof-samples.txt`） |
| Files Host stdin EOF | 5 | 3.9 ms | 37.7 ms | ≤2 s ✅（`files-eof-samples.txt`） |
| App Runtime stdin EOF | 5 | 1.3 ms | 337 ms | ≤2 s ✅（`app-eof-samples.txt`） |

测试脚本与原始样本保留于 `.tmp/host-lifecycle/`（脚本可复现：隔离 HOME、独立 namespace，不触碰真实用户数据/Keychain）。

## L01～L16 逐项结果

| 编号 | 场景 | 结果 |
|---|---|---|
| L01 | 100 短 helper 回收与竞态 | ✅ `TestLaunchReapsShortHelpers`（自然退出/启动失败/取消/输出失败并发；僵尸扫描零残留） |
| L02 | 单实例锁/socket | ✅（Unix 既有语义保持；`instance_unix_test` + 模块测试）；Windows 语义编译级验证，实机未验 |
| L03 | 非常驻退出/常驻保持/浏览器退出 | ✅ 既有 process 测试 + main_test 通过；EOF 实测见上 |
| L04 | 客户端上限/慢客户端/字节上限 | ✅ `TestClientAdmissionLimit`、`TestSlowClientRemovedWithoutBlockingOthers`、`TestBroadcastMergesSameEventPerClient` |
| L05 | Accept/Read/Write 阻塞退出 | ✅ accept 联动 + `TestCloseAllUnblocksReaderAndPump`；写路径由 deadline/RAII 覆盖 |
| L06 | 采集中取消/预算 | ✅ `TestCollectSourcesRespectsCancellation`、`TestFindFilesWithExtBudgets` |
| L07 | owner 断连只取消其任务 | ✅ `TestCancelInferenceForOwners`、`TestInferenceOwnerExtraction` |
| L08 | 用量容量/背压/字节 | ✅ SDK `TestManagerCapacityIsBounded`、`TestManagerQueueBytesBounded` |
| L09 | 满队列 close/竞态/50 次启停 | ✅ SDK `TestManagerStopDrainsAndDoesNotResurrect`、`TestManagerConcurrentPublishStopRace`、`TestManagerRestartCyclesBaseline`；DB busy 场景由既有 store 测试覆盖 |
| L10 | 退出期限 | ✅ 实测见上（debug 构建；Release 对比未做，见未验证项） |
| L11 | dev 只清自有/冲突阻止 | ✅ self-test + 代码审查；真实双会话冲突场景未模拟 |
| L12 | Windows 隔离 | ⚠️ 代码完成 + 交叉编译通过；真实 Windows 未验证（blocked） |
| L13 | 更新按钮/75 重启/单 reader 回归 | ✅ 既有 `TestBridgeReadsChromeInputOnceAcrossWorkerRestart` 及 kernel 测试全部通过；真实 Chrome 换装未点击（沿用既有边界） |
| L14 | Files 工具超时/reap | ✅ 代码 + 编译 + 既有 75 项测试；pdftotext/mdfind 有界路径就位；真实超时故障注入未单独跑（blocked 于故障注入夹具） |
| L15 | App 重复 shutdown/50 次开关 | ✅ 既有 lifecycle（7）/deadline（3）测试通过（含 50 次打开关闭与 2s 契约、watchdog 语义）；看门狗去重新增逻辑由重复 shutdown 路径覆盖 |
| L16 | Release 对比/30 分钟 Chrome 循环 | ⚠️ 未做（见下） |

## 未验证 / 阻塞项（如实区分）

1. **L16 Release before/after 与 30 分钟真实 Chrome 循环未执行**——需要长时真机采样窗口与 Release 采样入口扩量；本轮以 debug 构建 EOF 实测 + 全量测试门禁为本地证据，不宣称 Release 对比通过。
2. **Windows 实机（L12）**——管道命名空间代码完成并通过交叉编译，真实 Windows 启动/隔离/ACL 未验证（无设备），标 blocked。
3. **macOS Intel**——本机为 arm64，未在 Intel 真机复测。
4. **真实 Chrome 全流程（L13 更新按钮点击换装）**——未点击真实页面更新按钮，沿用既往「真实换装未验」边界。
5. **慢客户端真实 socket 故障注入（L04 的 socket 层）**——以 net.Pipe 与 deadline 逻辑覆盖；未做真实 loopback TCP 慢客户端长测。
6. **model:host:check 中 `internal/usage/sources.go` 的 gofmt 偏差为先前已存在**（非本次改动文件，按修改边界不顺手重排，已记录）。

## 边界保持声明

- 未新增 daemon/系统服务/登录启动项/第二 IPC/第二数据权威；lifecycle 包仅容纳有真实调用方的原语。
- 未按名称/路径/PPID 批量杀进程；dev 清扫改为会话内身份核验。
- 未手动升级内核、未实施 v8 迁移（依赖仍 v7.3.17；补丁重放以 v7.3.17 为基线）。
- 未修改真实用户数据、Keychain、生产注册、签名产品源；测试使用隔离 HOME 与 memory/独立 namespace。
- 未提交、未推送、未安装、未发布。
