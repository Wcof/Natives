# 05 - 本地服务端口注册表（Local Service Port Registry）

> 状态：生效中（enforced）
> 依据：ADR-0032《App Runtime 固定默认端口与本地服务端口注册表》
> 本文档是 Natives 全部本地 loopback 监听端口的**唯一登记台账**。任何代码引入、变更或退役一个本地监听端口，必须先在此登记/更新，并在对应 ADR 中说明理由。

## 1. 分配区间与规则

- Natives 本地服务仅使用 **8765–8799** 区间，且一律绑定 `127.0.0.1`（严禁绑定 `0.0.0.0` 或非回环地址）。
- 新服务申请端口：在本文档追加条目 + 立 ADR 说明理由；冲突时以本表仲裁。
- 未登记的监听端口视为违规，评审与检查应拒绝。

## 2. 注册表

| 端口 | 服务 / 二进制 | 协议与路径前缀 | 鉴权 | 生命周期 | 消费方 | 依据 |
|------|---------------|----------------|------|----------|--------|------|
| 8765 | `natives-app-runtime` loopback HTTP | HTTP/1.1，`/`（模块 UI 静态资源）、`/api/*`（业务 API） | UI 静态资源免鉴权；业务 API 需沙箱 `Origin: null` + Bearer Token（`app:session issue` 签发，绑定 generation） | 会话式（Native Messaging 连接持有，stdin EOF / 页面关闭 ≤2s 退出） | Chrome 扩展沙箱 iframe（经 `StartResult.port`）；进程外只读消费者（顶栏 `natives-statusbar` 轮询 `/api/tray/state`、诊断脚本、测试 harness） | ADR-0032 |
| 8766 | `model-host` 只读 AI 数据查询 | HTTP/1.1，`GET /ai/usage/*`、`GET /ai/sessions/*`、`GET /ai/quota`、`POST /ai/quota/refresh`（唯一上游触发口） | 回环 + Host 头校验 + 尺寸/并发/速率上限；无 Bearer 会话；响应禁含 Secret；不返回 CORS 头 | 随 model-host 进程生命周期（非常驻时不可达为常态） | 进程外只读消费者：顶栏 `natives-statusbar`、诊断脚本、测试 harness；扩展不改走此端口（仍走 Native Messaging） | ADR-0033 |
| ~~8795~~ | ~~`natives-app-runtime`（fund 模块伴生趋势引擎）~~ | ~~HTTP/1.1，`/api/*`~~ | — | **已退役**（2026-09-28 缠论与趋势算法完成 Rust 纯计算组件移植，`engine/` 目录与伴生进程已彻底删除，无服务监听） | — | 审计 F-03/A1/A4 |

## 3. 回退与覆盖语义（8765 特有）

- 覆盖：环境变量 `NATIVES_APP_RUNTIME_PORT`（设为 `0` 强制动态端口）。
- 回退：8765 被占用时自动回退动态端口（`:0`），实际端口经 `app:start` 响应 `StartResult.port` 上报给扩展页面；固定端口服务于第一个就绪的实例，供进程外消费者对接。
- 轮询方须将「端口不可达」视为常态（运行时进程未运行）。
