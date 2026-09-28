# ADR-0034：Google Antigravity 公共 OAuth client 凭据的豁免登记（Google Antigravity Public OAuth Client Credentials Exemption）

- 状态：已接受（accepted，2026-09-28）
- 决策者：产品方（用户）
- 关联文档：ADR-0020（R-S4 Secret 边界）、ADR-0033、审计行动项 E1/S-09、`docs/standards/technical/02-security.md`

---

## 1. 背景与问题

审计 S-02/S-09 指出 Google OAuth 的 `client_id`/`client_secret` 以明文常量出现在两处：
ObjC 顶栏（`NativesStatusBarData.h`，已随 B2 删除）与 Go 侧
（`model-host/internal/host/authfiles_handlers.go` `antigravityOAuthClientID/Secret`），
要求"迁 Keychain/受控注入或立 ADR 豁免并说明依据"。

核实事实：这对凭据是 Google **公开发布的 OAuth 客户端标识**（Antigravity/Code Assist
CLI 的公共 client），与 `third_party/cliproxyapi/internal/runtime/executor/`
`antigravity_executor.go` 内嵌上游代码中的常量**逐字节相同**，且随上游开源分发给全部
使用者。它不是用户凭据，也不构成 Secret：OAuth 公共 client 的 client_secret 本就不
具备保密性（PKCE/refresh token 才是凭据边界，后者始终只在 OS Keychain，R-S4）。

## 2. 决策

1. **不入库注入**：仓库（含 fork vendored 代码）一律只保留占位符
   （`OAUTH_CLIENT_ID_PLACEHOLDER` / `GOCSPX-LOCAL-ONLY-REDACTED`），避免 GitHub
   Push Protection 将公共 client 凭据误判为 Secret 拦截推送；真实值通过
   `NATIVES_ANTIGRAVITY_OAUTH_CLIENT_ID` / `NATIVES_ANTIGRAVITY_OAUTH_CLIENT_SECRET`
   环境变量在本机注入（`model-host/.env.local`，git 忽略），不入库、不被 git 追踪。
   上游升级导出 fork 时须按 `natives-patches/` 规范重放占位化补丁。
2. **单一份留存**：ObjC 侧副本已随 B2（顶栏退役第三套实现）删除；Go 侧为唯一注入点。
3. **轮换评估结论**：公共 client 凭据由 Google 发布、公开分发，无需且无法"轮换"；
   需要保密的用户 refresh_token/access_token 已全链路只在 OS Keychain（R-S4 合规）。
   若 Google 未来更换公共 client，仅需更新本机 `.env.local` 并随上游 CLIProxyAPI
   版本升级同步（分体式同步规范）。
4. **降级行为**：未配置环境变量时，antigravity OAuth 刷新端点返回显式配置错误
   （而非静默使用占位符请求 Google），错误信息指引配置方式。

## 3. 后果

- 正面：E1 关闭（仓库内不再有"看起来像用户 Secret"的误导性硬编码面；顶栏副本清零）；
  明确了公共 client 与用户凭据的边界，后续审计不再重复标记。
- 代价：常量仍出现在仓库明文中（与上游一致，属公开事实）；此豁免仅限这一对凭据，
  不得扩大到任何用户级凭据。

## 4. 修订记录

- 2026-09-28：初稿。依据审计行动项 E1/S-09 与产品方"按推荐执行"拍板。
