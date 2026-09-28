# AI 用量/额度/会话查询收口方案

状态：**已评审通过（2026-09-27，产品方拍板）**。决策记录：①Phase 0+1 一起实施；
②Phase 2 立案 ADR-0033（model-host 只读查询端口 8766，已登记端口注册表）。
Phase 2 的端口与路由实现待 ADR-0033 对应代码落地后生效。

**实施状态（2026-09-28 更新）**：Phase 0 全部完成（含 0.4 采集触发点——
`TriggerCollectAsync` 于 NM 连接建立时事件驱动采集，10s 防抖，无定时器）。
Phase 1 完成，且 ZCode 权威载体已收敛：model-host 采集器优先读
`~/.zcode/cli/db/db.sqlite`（parser `zcode-sqlite/1`，毫秒游标增量），
rollout JSONL 降为兜底；权威库已回填 16,591 条真实 ZCode 事件；
顶栏保留一段**守卫式** ZCode 兜底（仅当统一库无 zcode 数据时生效，回填后
恒为休眠态），随 Phase 2 的 B2 顶栏退役一并移除。

## 1. 背景与目标

tokenusage 域（AI 用量、额度、请求会话、技能统计）当前有三套并行实现、四类消费方
零散直连。后续订单模块与其他内置模块也要消费同一批数据。本方案的目标：

1. **一个写入权威**：同一份数据只有一条采集与写入库的路径。
2. **按传输域划分的查询门面**：扩展、子应用、进程外消费者各有一个明确入口，
   不允许新消费者再自建直连。
3. **口径一致**：同一指标在任何消费方面前数值一致（含"零假数据"边界）。
4. **可扩展**：会话、技能等新查询维度落在同一权威与同一门面内，不再新增旁路。

## 2. 现状核实（审计结论）

### 2.1 三套并行实现

| | Go（model-host） | Rust（tokenusage 模块） | ObjC（顶栏） |
|---|---|---|---|
| 进程 | model-host（NM host，可常驻） | natives-app-runtime（按需，≤2s 退出） | natives-statusbar（常驻） |
| 数据库 | `~/.natives/usage.db`（权威，读写字） | `~/.natives[-local]/apps/tokenusage/data/tokenusage.db`（投影缓存+自采） | 不拥有库，直读前两者 + 直扫会话文件 |
| 采集 | `internal/usage/collector*.go`：8 个根目录（zcode rollout、codex sessions+archived、claude、pi 等） | `collector/scanner.rs`：30 个源直扫 + `collector/host_sync.rs` 只读同步 usage.db | `NativesStatusBarData.h` 直扫 codex/pi + 直读 `~/.zcode/cli/db/db.sqlite` |
| 上游额度 | `internal/quota/client.go`：codex/claude/antigravity/xai/kimi 五家，含"假视图"收敛 | 无（只读 state.json 标身份） | `FetchCodexUsageLive`/`FetchAntigravityUsageLive` 重复实现 codex/antigravity 两家 |

### 2.2 现有消费方与各自通道

| 消费方 | 通道 | 说明 |
|---|---|---|
| 扩展·空间页 AI 效能组件（7+1 卡片） | Native Messaging `model_usage_*` → model-host | 已经过 `extension/tokenusage/` 共享层收口（单 Port、同参合并、推送失效） |
| 扩展·服务与模型对话框（用量页/OAuth 额度页） | 同上 | 与效能组件共享查询层；额度查询 `model_quota_query` 手动触发 |
| 顶栏状态栏 | `GET 127.0.0.1:8765/api/tray/state`、`/api/limits`（ADR-0032 豁免）+ SQLite 直读兜底 + 会话文件直扫 + 上游额度直连 | 绕开前两条管道的主因：tokenusage.db 聚合"依赖采集器运行，长期为 0"（`NativesStatusBarData.h:811` 自述） |
| 子应用（fund 等） | 8765 沙箱 Bearer 会话 | 目前未消费 AI 域数据，属未来消费方 |

### 2.3 口径差异矩阵（同一数据的多头实现）

| 数据 | Go | Rust | ObjC | 差异要点 |
|---|---|---|---|---|
| Codex 会话用量 | rollout + **archived_sessions**，事件级游标增量 | scanner 全量解析 + record_hash 去重 | 尾读 256KB、thread 取最大累计，**不读 archived** | 三套解析器；累计口径 vs 事件口径 |
| ZCode 用量 | `cli/rollout/*.jsonl` | `~/.zcode/projects`、`~/.zcode/cli` 扫描 | `~/.zcode/cli/db/db.sqlite` model_usage 表 | **三个进程读三种不同载体** |
| 成本计价 | 定价目录（usage.db model_prices） | 定价目录（pricing.rs） | 启发式 `in*1.5+out*6` | 顶栏数值与权威库不一致 |
| 每日/周期聚合 | usage_events 实时聚合 | daily_aggregates 预聚合 | daily_aggregates SQL + 直扫结果覆盖 | 顶栏为"DB+直扫"超集，DB 行小时被覆盖 |
| 工具分解 / 最近会话 / 趋势 | GetAnalytics / GetSessions | panel_json | 逐字复制的 SQL + 额外直扫合并 | A/B SQL 逐字一致，ObjC 再叠加私有通道 |
| Codex/Antigravity 额度 | 三端点回退 + Keychain 续期 + 写 quota-cache.json | 无 | 仅生产端点 + Keychain 续期 + **第二个 quota-cache.json 写方** | 假视图收敛逻辑两份近逐行等价实现；client_id/secret 硬编码两份；两写方无互斥 |
| 会话定义 | usage_events 会话归属（ADR-0030 billingAtom/direct_owner） | sessions 表自累加 | 直扫文件按 thread/session | "什么是会话"没有统一答案 |

### 2.4 与 ADR 的冲突

- **ADR-0030**（`docs/adr/0030-ai-usage-ledger-and-collection-boundaries.md:11,27`）明文：
  `model-host/internal/usage` 是用量/计费/预算/提醒的**唯一写入权威**、**不建第二用量库**。
  tokenusage 模块自带的 30 源扫描器直接写第二个用量库，是现状中最大的架构偏差。
- **ADR-0032**（§2.1.5）：不为顶栏放宽 Bearer；只读豁免（tray/state、limits）已立此 ADR，
  再新增豁免须另立 ADR。
- **02-security R-S7**：8765 业务 API 的 CORS 只允许 `Origin: null`。扩展页面是
  `chrome-extension://` 源，**永远不能直连 8765**——扩展走 Native Messaging 是架构必然，
  不是历史包袱。

## 3. 目标架构

```
                    ┌─ 采集（唯一）────────────────────────────┐
  本地会话文件 ────▶ │ model-host/internal/usage（权威 usage.db）│ ◀── 工具 hook 事件
  上游额度端点 ────▶ │ model-host/internal/quota（探测+缓存）    │      (model_tool_event_ingest)
                    └──────────────┬───────────────────────────┘
                                   │ 只读（host_sync 投影 / 只读契约）
              ┌────────────────────┼──────────────────────────┐
              ▼                    ▼                          ▼
   【门面 A】NM 域          【门面 B】Loopback 域           【消费者 C】进程外只读
   model_usage_* 方法族     tokenusage 模块 /api/*（8765）   顶栏：tray/state+limits 豁免路由，
   → 扩展共享层             → 沙箱子应用（现有/未来模块）      兜底=只读库；Phase2 增 model-host
   （extension/tokenusage）                                  只读查询端口（需 ADR-0033）
```

**角色重定义：**

1. **model-host = AI 数据域唯一权威**（写）：本地采集、上游额度探测、事件摄入、
   billingAtom 归因，全部只在 Go 侧发生（ADR-0030 原意）。
2. **tokenusage 模块 = Loopback 域查询门面 + 投影缓存**（读）：`/api/*` 路由保留并成为
   子应用域的唯一 AI 查询入口；tokenusage.db 降级为纯投影缓存，唯一摄入通道是
   host_sync 只读同步；**scanner.rs 与 30 源直扫退役**。
3. **extension/tokenusage/ 共享层 = NM 域查询门面**（已收口，保持）：方法语义与门面 B
   对齐，死方法（`getUsageStatus`/`getUsageSubjects`/`collectUsage`）清理。
4. **顶栏 = 进程外只读消费者**：只在 ADR-0032 豁免路由 + 只读库兜底 +（Phase 2）
   model-host 只读端口上取数，退役全部直扫/直连/第二缓存写方。

**跨进程读取契约（给订单等未来模块）**：`usage.db` 的 SQLite schema 是稳定只读契约
（登记入 `docs/contracts/`），任何模块可以只读打开做投影（现 host_sync 即此模式）；
写路径永远不存在第二条。待第二个真实消费者（订单）落地时，再把 host_sync 的只读
读取端提取为共享 Rust crate（满足 R-B4"两个真实调用方"门槛，不预先抽象）。

## 4. 契约清单（收口的"收口点"）

| 契约 | 内容 | 校验方式 |
|---|---|---|
| usage.db 只读契约 | 表结构、语义（事件/会话/预算/归因）、只读规则 | 文档化 + Go 侧 migration 前向兼容纪律 |
| `model_usage_*` 方法契约 | NM 方法白名单、参数、返回形状、推送事件（`model_usage_updated`） | Host 边界白名单（已有）+ 扩展共享层类型定义 |
| tokenusage `/api/*` 契约 | 路由、Bearer 豁免范围（ADR-0032 两豁免不再扩大）、字段 | 契约测试 |
| tray/state 口径契约 | panel_json 字段与顶栏 SQLite 兜底逐字段一致（既有规范延续） | 同库同输出的对照测试（Rust vs ObjC fixture） |
| 会话/技能语义 | 会话归属与 Skill 归因遵循 ADR-0030 billingAtom（`direct_owner` 汇总） | Go 侧单测 |

## 5. 分阶段实施

### Phase 0 — 无 ADR 即可做（纯收敛，不改边界）

1. **退役 tokenusage 模块自带扫描器**：删除 `collector/scanner.rs` 的业务调用
   （保留 host_sync），`collect_all` 只走 host_sync。这使现状回到 ADR-0030 轨道。
2. **Go 采集源补齐**：将 Rust 30 源清单中确有真实数据的源并入 Go collector
   （8 → 对齐），含 codex archived_sessions 全量口径；ZCode 统一为 rollout jsonl。
3. **口径清理**：顶栏启发式计价（in*1.5+out*6）随 Phase 1 兜底改造一并退役；
   panel_json 冗余字段（`overview.periods` 与四周期重复）清理；扩展死方法清理；
   顶栏残留代码（未使用的 NSURLSession、注释与实现不符的轮播周期）清理。
4. **采集触发点补齐（不做定时器，遵守 ADR-0030"无常驻采集进程"）**：model-host 在
   NM 连接建立时与显式 `model_usage_collect` 时执行本地文件采集（纯本地文件读，
   无上游请求）；tokenusage 模块 start() 采集保留。由此消除"顶栏看到长期为 0"的根因。

### Phase 1 — 顶栏兜底收口（仍无新 ADR，沿用 ADR-0032 既有豁免）

1. 顶栏 SQLite 兜底改为**只读 tokenusage.db**（与 tray/state 同库同口径），
   删除 `FetchNativeSessionUsageByDate`、`ScanCodexSessionFile`、`ScanPiSessionFile`、
   ZCode db.sqlite 直读。
2. 上游额度实时请求**暂留 ObjC**（顶栏是唯一常驻进程，model-host 不保证在运行；
   这是当前唯一不产生体验回退的选择），但：假视图收敛语义与 Go 保持同一份
   （已完成），quota-cache.json 过渡期为"同逻辑双写方"，列入 Phase 2 消除。
3. 建立口径契约测试（tray/state vs ObjC 兜底；host_sync 幂等性）。

### Phase 2 — 单点化与常驻查询面（需 ADR-0033 + 端口注册表登记）

1. **ADR-0033**：model-host 新增只读 loopback 查询端口（8766，绑定 127.0.0.1，
   Host 头校验 + 速率上限，R-S5 约束；不暴露 Secret/管理端点），提供：
   用量聚合查询、会话/技能查询、额度读取、**手动刷新触发**（额度上游请求仍只在
   用户显式动作时发生）。
2. 顶栏退役 ObjC 直连上游额度（`FetchCodexUsageLive`/`FetchAntigravityUsageLive`、
   Keychain 直读与续期、quota-cache.json 写方），改为：Phase 2 端口可达时取数，
   不可达时读 quota-cache.json + limits_cache 并显式标注"模型服务未运行"。
   quota-cache.json 归属 Go 单一写方。
3. 顶栏保留 ADR-0032 两豁免路由作为 tray 数据主通道不变（8765 与 8766 皆不可达时
   才落 SQLite 兜底）。

### 各消费方最终形态

| 消费方 | 收口后取数路径 |
|---|---|
| 扩展（空间效能组件、服务与模型） | Native Messaging `model_usage_*`（不变，已是门面 A） |
| 子应用沙箱 UI（现有/未来） | 本进程 tokenusage 门面 B `/api/*` |
| 订单等未来模块（Rust 侧） | 只读 usage.db 契约自建投影（host_sync 模式）；消费者≥2 时提取共享读取 crate |
| 顶栏 | tray/state + limits（8765）→ 8766 只读查询 → 只读库兜底，三级降级 |
| 诊断/测试 harness | 同顶栏的只读路径，禁止新增直连 |

## 6. 需要的决策与 ADR

1. **ADR-0033（Phase 2 前置）**：model-host 只读查询端口 + 顶栏作为其进程外消费者；
   端口注册表追加条目。这是对 ADR-0032"不放宽"立场的**新增豁免面**，必须走 ADR。
2. **ADR-0030 备忘更新**（非放宽）：记录 tokenusage 模块降级为投影、扫描器退役、
   采集触发点清单（事件驱动、无定时器）。
3. **订单模块接入方式的确认**：只读契约投影 vs 共享读取 crate，等订单立项时定。

## 7. 非目标

- 不建通用统计/OTel 平台、不建通用任务系统（ADR-0030 已拒绝）。
- 不合并 `com.natives.model_host` 与 `com.natives.app_runtime` 两个 Host，
  不改扩展页面的 CORS/鉴权模型。
- 不动 LLM 网关（8317）与 CLIProxyAPI 管理边界（ADR-0020）。
- 不在本方案内实现订单模块或技能统计的具体采集（只预留语义位与查询位）。

## 8. 风险与对策

| 风险 | 对策 |
|---|---|
| Go collector 扩源工作量与解析正确性 | 按 Rust scanner 的解析器与测试移植，逐源对照真实文件验收 |
| 顶栏对"采集已发生"的依赖上升 | 三级降级保留只读兜底；采集触发点覆盖"浏览器使用即采集" |
| usage.db schema 冻结影响 Go 迭代 | 契约只承诺已发布表的前向兼容，新增列允许、破坏性变更须更新契约文档 |
| Phase 2 前 quota-cache 双写方并存 | 两写方逻辑已同源（假视图收敛一致）；过渡期接受，Phase 2 单写方收口 |
| 会话/技能语义争议 | 以 ADR-0030 billingAtom 为唯一裁决依据，契约文档落字 |

## 9. 验收标准

1. 全仓检索不再出现第三处 AI 用量聚合实现（ObjC 直扫/直连清零，或仅剩 Phase 1
   过渡项且有 ADR 关联）。
2. 同一时刻、同一账号，扩展用量页、空间效能卡片、顶栏面板、tokenusage 子应用
   四处数值一致（同一权威库时刻的投影）。
3. 顶栏在"浏览器从未打开"的最坏情况下仍显示真实（允许较旧）数据并明确降级来源，
   不出现 0 或占位。
4. 契约测试（tray/state 口径、host_sync 幂等、方法白名单）进入门禁。
