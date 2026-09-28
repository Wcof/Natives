# ADR-0033：Model Host 只读 AI 数据查询端口（Model Host Read-Only AI Query Port）

- 状态：已接受（accepted，2026-09-27），实施归属收口方案 Phase 2
- 决策者：产品方（用户）
- 关联文档：ADR-0020、ADR-0030、ADR-0032、[AI 用量/额度/会话查询收口方案](../development/tokenusage-consolidation-plan.md)、`docs/standards/technical/02-security.md`、`docs/standards/technical/05-port-registry.md`

---

## 1. 背景与问题

收口方案核实（2026-09-27）：AI 用量/额度数据存在 Go（model-host）、Rust（tokenusage
模块）、ObjC（顶栏）三套并行实现。扩展与子应用各有一个受控查询门面（Native
Messaging 方法族 / 8765 模块路由），唯独**进程外常驻消费者（macOS 顶栏）没有任何
可对接的 AI 数据查询面**：

1. 8765（app-runtime）是会话式进程，随页面关闭 ≤2s 退出，顶栏大部分时间无服务可查；
2. 因此顶栏自建了第三套实现：直扫 `~/.codex`/`~/.pi`/ZCode 会话文件、直连上游额度
   端点（Codex wham、Google cloudcode-pa）、直读 Keychain 并续期、自写
   `quota-cache.json`（与 Go 形成无互斥双写方）。同一份数据三处口径不一致。

ADR-0032 已明确"不为顶栏放宽 Bearer Token 鉴权，只读豁免另立 ADR"；本 ADR 即该
预留的决策点。同时 ADR-0030 确立 model-host/internal/usage 为用量域唯一写入权威，
但权威库（`~/.natives/usage.db`）没有任何 HTTP 查询面，权威地位落不了地。

## 2. 决策

### 2.1 新增只读查询端口 8766

1. `model-host` 新增 **只读 AI 数据查询 loopback HTTP 服务**，绑定 `127.0.0.1:8766`
   （端口注册表顺序分配），随 model-host 进程生命周期启停。
2. 接口面（只读 + 单一显式触发，全部窄接口）：
   - `GET /ai/usage/*`：用量聚合、会话、事件、来源、预算、账单、提醒、建议、定价
     （与 Native Messaging `model_usage_*` 方法族同一数据权威与同一聚合口径）；
   - `GET /ai/sessions/*`：请求会话/技能归因查询（ADR-0030 billingAtom 口径）；
   - `GET /ai/quota`：读取最近一次额度探测结果（quota-cache，内存或缓存文件）；
   - `POST /ai/quota/refresh`：**唯一**触发上游额度探测的入口，仅由用户显式动作
     （顶栏刷新按钮等）调用；除此之外该端口不存在任何会发起上游请求的路由。
3. **安全边界（R-S5 对齐，全部 MUST）**：
   - 仅绑定 `127.0.0.1`，校验 `Host` 头；不返回 CORS 头（无浏览器消费场景）；
   - 请求/响应尺寸上限、并发与速率上限；日志脱敏；
   - 响应中**严禁出现** access/refresh token、API key、凭据文件内容等 Secret
     （R-S4：只允许百分比、时间戳等派生值）；
   - 不暴露 CLIProxyAPI Management API、任意文件、进程、SQLite 任意查询能力
     （ADR-0020 边界不变）；
   - 不做 Bearer 会话（消费方为进程外本地组件，与 8765 的沙箱会话模型不同；
     以回环 + Host 头 + 上限 + 最小数据面为补偿控制）。
4. **生命周期与可用性**：端口随 model-host 存活；model-host 非常驻时不可达是常态。
   消费者必须实现降级链：8766 可达 → 只读库（usage.db/tokenusage.db 投影）→
   显式"模型服务未运行"空态。**严禁**消费方因此自建上游直连（该旁路正是本 ADR
   要退役的对象）。

### 2.2 顶栏消费通道收口（本 ADR 的直接后果）

Phase 2 实施时，顶栏（`NativesStatusBarData.h`/`NativesStatusBar.m`）退役：
`FetchCodexUsageLive`、`FetchAntigravityUsageLive`（含 Keychain 直读与续期、
quota-cache.json 第二写方）、`FetchNativeSessionUsageByDate` 及全部会话文件直扫、
ZCode `db.sqlite` 直读。顶栏取数收敛为三级降级：8765 `tray/state`+`limits`
（ADR-0032 豁免不变）→ 8766 查询 → 只读库兜底。

### 2.3 与 ADR-0030 的关系

本 ADR 不改变 ADR-0030 的唯一写入权威与采集边界：8766 全部为只读投影；
唯一的上游额度探测点仍在 `model-host/internal/quota`；不引入定时器与常驻采集。

## 3. 后果

- 正面：AI 数据域"单权威 + 按传输域分面"闭环（扩展=NM、子应用=8765、进程外=8766）；
  顶栏第三套实现可整体退役；订单等未来进程外消费者有现成对接面。
- 代价与缓解：新增一个本地端口与一份路由面（登记于端口注册表，受 02-security 约束）；
  model-host 未运行时顶栏额度显示降级为缓存/空态（零假数据，可接受）。
- 明确不做什么：不为 8766 引入 Bearer/会话体系；不让扩展改走 8766（其通道仍是
  Native Messaging）；不提供任何写路由（除显式额度刷新触发）。

## 4. 修订记录

- 2026-09-27：初稿。依据收口方案（Phase 2 决策：产品方选择"立 ADR-0033"）。
