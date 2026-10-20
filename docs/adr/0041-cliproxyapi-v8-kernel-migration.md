# ADR-0041 · CLIProxyAPI v8 内核迁移

- 日期：2026-10-14
- 状态：接受（用户明确要求更新路由模块内核）
- 取代范围：ADR-0020 §6.1 中固定 CLIProxyAPI v7 提交与模块路径的描述；其余 Model Host、Secret、loopback、生命周期和产品边界不变。
- 依赖：ADR-0020、ADR-0034、ADR-0037、ADR-0040。

## 背景

Model Host 使用 `CLIProxyAPI v7.3.17` 的裁剪源码 fork。上游最新稳定标签
`v8.0.28` 已将 Go 模块路径迁移至 `github.com/router-for-me/CLIProxyAPI/v8`，
原有开发更新器因此正确拒绝跨主版本原地换装。用户要求完成这次 SDK 与本地补丁迁移。

## 决策

1. 将裁剪 fork 的上游基线固定为 `v8.0.28`、提交
   `0d18d3b365be000eeaee7de4615f9da596b2059d`，仍只保留 `internal/`、
   `sdk/` 和 Go module 元数据，不引入上游 CLI、Management UI 或第二配置权威。
2. Model Host 的 SDK import、require 与本地 replace 统一切换到 v8 模块路径。
   现有 Keychain、自定义 auth Store、loopback 鉴权和 Provider 配置 owner 不变；
   不迁移或改写用户凭据与状态文件。
3. 重放并重新生成 Natives 补丁：有界可重启 usage dispatcher、Antigravity
   临时区域错误、静态模型桥接、编译时版本，以及兼容供应商忽略流请求时的
   JSON→SSE 转换。usage 补丁须保留 v8 新增的 API Key 来源与 native-key 元数据。
4. 开发内核更新器改为验证 v8 模块路径，并继续只接受同主版本稳定标签。
   未来 v9 等主版本仍返回 `kernel_migration_required`，不得绕过产品迁移审查。
5. 正式产品仍通过完整 Natives 验签与更新链交付；源码按钮只服务受核验的
   本地开发身份，不扩大远程更新或执行面。

## 验证边界

必须验证补丁可在干净 `v8.0.28` 导出上重放、Model Host 全包编译与测试、
网关启动和模型列表、Extension／生产集成门禁。上游标签自身失败的测试单独记录，
不得修改断言、放宽安全检查或把上游失败改写为通过。真实供应商验证不发送用户数据。
