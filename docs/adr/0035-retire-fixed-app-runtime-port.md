# ADR-0035：退役 App Runtime 固定 8765 端口，恢复会话级动态端口（Retire Fixed App Runtime Port 8765 in Favor of Dynamic Ephemeral Ports）

- 状态：已接受（accepted，2026-09-28）
- 决策者：产品架构团队
- 修订：ADR-0032（修订其 App Runtime 固定端口 8765 分配方案，将 8765 标记为 retired）
- 保留：ADR-0032 建立的“本地固定端口必须集中登记于 `05-port-registry.md`”的核心治理原则
- 关联文档：ADR-0031（统一运行时与单进程实例模型）、ADR-0032（端口治理）、ADR-0033（Model Host 8766 只读查询端口）、ADR-0034（凭据豁免）、`docs/standards/technical/05-port-registry.md`

---

## 1. 背景与问题

ADR-0032 曾为进程外消费者（如 macOS 状态栏）引入 App Runtime 默认绑定固定端口 8765。但在当前 Natives 架构中，内置子应用（Built-in App）的运行模型是：
1. 一个统一的 `natives-app-runtime` 二进制文件；
2. 每个活动的内置子应用（如 Fund、TokenUsage）独立拥有一个 `natives-app-runtime` 进程实例（Single Runtime Binary, Multi Process Instance）；
3. 每个进程实例仅绑定一个具体模块（通过 `app:handshake` 确定 `appId`）。

当用户同时打开多个内置应用页面时，产生严重的端口冲突与语义歧义：
- 第一个启动的进程占据了 8765 端口；
- 后续启动的进程因为端口冲突回退到动态端口；
- **8765 端口背后的业务身份完全取决于哪个应用最先被用户打开**：如果用户先开 Fund，8765 是 Fund；如果用户先开 TokenUsage，8765 才是 TokenUsage。外部组件无法预知 8765 代表谁。
- 为让外部组件访问 8765，代码中对 `/api/tray/state` 和 `/api/limits` 放宽了 Bearer Token 与 sandbox Origin 鉴权，直接破坏了 ADR-0032 声明的“固定端口不放宽 Bearer”安全承诺。

同时，ADR-0033 已经正式落地并在生产中启用了 `model-host:8766` 只读 AI 查询端口，为外部消费者（包括 macOS 顶栏与监控组件）提供了稳定、具备唯一数据所有权（Model Host 权威持有 `usage.db` 与 Keychain）的查询通道。8765 外部消费的最主要场景已被 8766 完美取代。

## 2. 决策

### 2.1 Release App Runtime 恢复纯动态 Loopback 绑定
- 正式发布环境下的 `natives-app-runtime` 在启动 HTTP 服务时，一律绑定 `127.0.0.1:0`（操作系统动态分配可用端口），不再主动尝试绑定 8765。
- 实际分配的动态端口**仅通过** Native Messaging 握手通信帧的 `StartResult.port`（`app:start` 响应）暴露给当前的 App Owner Page（`extension/app.html`）。

### 2.2 8765 固定端口正式退役（Retired）
- `docs/standards/technical/05-port-registry.md` 将 8765 标记为 `retired`，从 active 端口白名单中移除。
- Release 构建代码中彻底删除尝试绑定 8765 的逻辑。
- 消除“第一个启动的 Runtime 是特权实例”的错误语义，所有 Built-in App Runtime 进程保持对称。
- 测试环境（test harness）若需固定端口，仅允许通过测试专属配置显式注入，绝不污染生产 Release 默认行为。

### 2.3 App Runtime API 恢复统一严格会话鉴权
- 废除 `/api/tray/state` 与 `/api/limits` 的无 Bearer / 无 sandbox Origin 免检特权。
- App Runtime HTTP 服务的所有 `/api/*` 路由一律强制要求：
  1. `Host: 127.0.0.1:<port>` 或 `localhost:<port>`；
  2. `Origin: chrome-extension://<id>` 且匹配当前已配置的 extension ID；
  3. `Authorization: Bearer <token>` 且 token 必须匹配当前 runtime 实例的会话 generation。
- 静态 UI 资源与基础无敏感 `/healthz` 探测继续支持无 token 访问。

### 2.4 外部查询按领域归属访问唯一权威接口
- AI 用量与额度查询：统一访问 `http://127.0.0.1:8766`（Model Host）。
- 未来其他需要进程外固定端口的领域：必须由真正的数据 Owner 建立独立 ADR 并在 `05-port-registry.md` 登记，严禁由通用 App Runtime 代劳。

## 3. 架构收益与影响

1. **多模块进程彻底对称**：每个子应用进程平等、独立分配动态端口，无启动竞态和端口抢占。
2. **安全边界回归**：彻底消除无鉴权 API 泄露风险，所有业务接口受 generation-bound Bearer 保护。
3. **职责唯一化**：AI 用量查询归于 Model Host（8766），投资领域归于 Fund（按需动态实例），各司其职。
4. **标准一致性**：彻底解决 `01-layering.md`、`02-security.md`、`05-port-registry.md` 与实际代码之间的历史冲突。

## 4. 修订记录

- 2026-09-28：初稿确立。依据架构审查 A-03 阻断项、ADR-0033 落地现状与产品架构规范正式接受。
