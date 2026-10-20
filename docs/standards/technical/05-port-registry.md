# 05 - 本地服务端口注册表（Local Service Port Registry）

> 状态：生效中（enforced）
> 依据：ADR-0032 的端口登记台账、ADR-0033 的 Model Host 查询端口，以及 ADR-0035 的 App Runtime 动态端口决策。ADR-0032 的固定 App Runtime 端口已退役。
> 本文档是 Natives 全部本地 loopback 监听端口的**唯一登记台账**。任何代码引入、变更或退役一个本地监听端口，必须先在此登记/更新，并在对应 ADR 中说明理由。

## 1. 分配区间与规则

- Natives 固定端口服务仅使用 **8765–8799** 区间；App Runtime 按 ADR-0035 使用系统分配的动态端口 `127.0.0.1:0`。所有服务一律绑定 `127.0.0.1`（严禁绑定 `0.0.0.0` 或非回环地址）。
- 新服务申请端口：在本文档追加条目 + 立 ADR 说明理由；冲突时以本表仲裁。
- 未登记的监听端口视为违规，评审与检查应拒绝。

## 2. 注册表

| 端口 | 服务 / 二进制 | 协议与路径前缀 | 鉴权 | 生命周期 | 消费方 | 依据 |
|------|---------------|----------------|------|----------|--------|------|
| ~~8765~~ | ~~`natives-app-runtime` loopback HTTP~~ | ~~HTTP/1.1，`/`、`/api/*`~~ | — | **已退役**（2026-09-28 ADR-0035 退役 App Runtime 固定 8765，恢复纯动态端口 `127.0.0.1:0`，消除多模块启动竞态与鉴权豁免） | — | ADR-0035 |
| 8766 | `model-host` 只读 AI 数据查询 | HTTP/1.1，`GET /ai/usage/*`、`GET /ai/sessions/*`、`GET /ai/quota`、`POST /ai/quota/refresh`（唯一上游触发口） | 回环 + Host 头校验 + 尺寸/并发/速率上限；无 Bearer 会话；响应禁含 Secret；不返回 CORS 头 | 随 model-host 进程生命周期（非常驻时不可达为常态） | 进程外只读消费者：顶栏 `natives-statusbar`、诊断脚本、测试 harness；扩展不改走此端口（仍走 Native Messaging） | ADR-0033 |
| 8770 | `native-file-host` 本机独立 Web 正式入口 | HTTP/1.1，`/`、`/web/*`、`/healthz` | 回环 + Host 头 + Bootstrap/配对码 + HttpOnly Cookie + CSRF + 精确 Origin | 单实例受 OS 锁保护，随最后活跃 Surface 租约释放后 60s 退出 | 无扩展普通 Chrome 浏览器、本机 Web 客户端 | ADR-0042 |
| 8771 | `native-file-host` 本机独立 Web 开发/测试入口 | HTTP/1.1，`/`、`/web/*`、`/healthz` | 同 8770（开发隔离 Cookie 与数据空间） | 开发与测试 harness 生命周期 | 本地开发与自动化测试 harness | ADR-0042 |
| ~~8795~~ | ~~`natives-app-runtime`（fund 模块伴生趋势引擎）~~ | ~~HTTP/1.1，`/api/*`~~ | — | **已退役**（2026-09-28 缠论与趋势算法完成 Rust 纯计算组件移植，`engine/` 目录与伴生进程已彻底删除，无服务监听） | — | 审计 F-03/A1/A4 |

## 3. App Runtime 动态端口语义（ADR-0035）

- ADR-0042 隔离测试例外：仅 `natives_dev` 构建、显式设置隔离 Model 配置根时，现有只读查询面可用 `NATIVES_MODEL_HOST_AIQUERY_PORT` 选择测试回环端口；查询客户端使用同一 URL。正式构建忽略此覆盖，默认端口仍是 8766，不新增产品服务或免鉴权业务口。

- `natives-app-runtime` 在发布环境下统一绑定操作系统分配的动态端口（`127.0.0.1:0`），不再尝试抢占 8765。
- 实际分配端口经 Native Messaging `app:start` 响应中的 `StartResult.port` 上报给扩展 App Owner Page。
- 业务 API 强制要求 sandbox Origin 与 Bearer Token，彻底消除 `/api/tray/state` / `/api/limits` 的免 Bearer 鉴权特权。
- 进程外 AI 用量与额度查询统一收口至 `model-host:8766`（ADR-0033）。
