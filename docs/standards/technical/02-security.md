# 技术 02 · 安全边界

> 版本：4.0.0 · 日期：2026-09-14
> 依据：ADR-0020、ADR-0023、ADR-0027、ADR-0029、托管应用契约

#### R-S1 · Native Messaging 注册最小化与单一 App Runtime 身份

- **等级**：MUST
- 系统原生 Native Messaging 注册收敛且固定：Files Host（`com.natives.file_manager`）、Model Host（`com.natives.model_host`）及 App Runtime Host（`com.natives.app_runtime`，本地开发 `com.natives.local.app_runtime`）。
- 严禁为各个内置应用动态生成或注册独立 Native Messaging Host（废除 `com.natives.app.a<hash>` 动态注册）。
- Manifest 只允许发布清单中的固定 Extension origin，路径指向受验签的产品可执行文件。
- Host 名称和路径由安装器/Core 计算；页面和模块声明不得覆盖。
- 修复/卸载只修改本产品拥有的收据和注册项。

#### R-S2 · Native 帧和方法白名单

- **等级**：MUST
- 长度前缀、最大帧、字段类型、字符串/数组长度和方法白名单在 Host 边界校验。
- 未知方法、未知字段、越界值、坏 JSON 和超大帧必须在产生副作用前拒绝。
- 写方法需要显式 request id、revision 或等价冲突保护；不得信任页面的 verified 标记。

#### R-S3 · 文件能力最小化

- **等级**：MUST
- Files Host 只允许用户授权 root 内的路径；所有 canonicalize、symlink/reparse point、`..`、绝对路径和竞态检查在 Host 完成。
- 页面不得获得任意进程、SQLite、Keychain 或明文 Secret 能力。
- 预览、搜索、导入和写入继续遵守尺寸、类型、并发与取消上限。

#### R-S4 · Secret 只由 OS Keychain 持有

- **等级**：MUST
- 持久 API key、OAuth token、refresh token 和模块 Secret 只进 OS Keychain。
- SQLite、JSON、前端 storage、产品清单、日志和错误只保存引用或掩码。
- Keychain locked/denied/unavailable 必须显示可恢复错误，不回退明文文件。
- 删除 Secret 是独立危险操作，必须二次确认并可重试。

#### R-S5 · Model Host 网络边界

- **等级**：MUST
- Local Proxy 只绑定 `127.0.0.1` 动态或受控端口并鉴权。
- 不暴露通用管理 API、任意文件/进程能力或企业多租户控制面。
- Provider 请求、OAuth callback 和工具探测必须有超时、取消、重定向/来源校验和日志脱敏。

#### R-S6 · App iframe sandbox 固定

- **等级**：MUST
- sandbox 精确允许 `allow-scripts allow-forms`。
- 禁止 `allow-same-origin`、顶层导航、弹窗、下载、cookie 和 credentials。
- `postMessage` 必须同时校验保存的 `contentWindow`、generation 和 challenge；`origin` 为 `null` 不是身份。
- iframe load/navigation、隐藏停止和页面关闭立即撤销旧 token。

#### R-S7 · App loopback 鉴权

- **等级**：MUST
- 每个按需启动的 App Runtime 进程只绑定 `127.0.0.1:0`，校验 Host header。
- 两阶段握手使用至少 128-bit challenge 和 32-byte CSPRNG bearer；token 最长 15 分钟并绑定 generation。
- CORS 只允许 `Origin: null`、声明的方法/头和有效 bearer。
- 单帧、请求体、响应、并发和速率必须有上限。

#### R-S8 · 产品级完整性

- **等级**：MUST
- 正式安装、更新和修复验证 Product Manifest、平台/架构、精确长度、双 SHA-256 和平台代码签名。
- 所有可执行代码属于 Natives Product Code（安装在系统产品源 `/Library/Application Support/Natives/hosts/` 下），用户应用数据目录（`~/.natives/apps/<appId>/`）严禁写入或保存任何 executable（废除 `runtime/<version>/app` 模式）。
- 模块代码随完整产品交付；不得从页面、Catalog、远程 URL 或模块脚本下载执行代码。
- macOS 正式载荷需要 Developer ID 与公证；Windows 需要 Authenticode；Linux 需要官方发布验签和权限控制。
- 不得移除 quarantine、关闭 Gatekeeper 或以 shell 绕过平台安全。

#### R-S9 · 开发身份隔离

- **等级**：MUST
- 本地模式使用独立产品身份、Extension ID、注册目录、数据根、Keychain namespace 和开发信任根。
- 环境变量或 fixture 字段不能把生产构建切成开发信任。
- 正式构建拒绝开发 key、fixture 和 ad-hoc 身份。

#### R-S10 · 日志与错误脱敏

- **等级**：MUST
- Authorization、token、API key、用户主目录和业务敏感内容写日志前统一脱敏。
- 页面只收到稳定错误 code、用户可理解信息和可执行 action；不暴露堆栈、SQL、命令或 Secret。

#### R-S11 · 浏览器安装现实边界

- **等级**：MUST
- 普通 Chrome 无法由本地应用静默安装开发者扩展时，Launcher 必须打开扩展管理页、随包扩展目录和本地指南。
- 不得声称已自动安装、降低 Chrome 安全策略或使用未支持的启动参数。

## 合规自检

- [ ] origin、Host 名称、路径和方法均由可信边界决定。
- [ ] Secret 只在 Keychain 和 Host 短期内存。
- [ ] iframe/loopback 两阶段鉴权与撤销完整。
- [ ] 模块没有远程代码和独立下载链。
- [ ] 开发与正式身份隔离。
- [ ] 日志与错误无敏感明文。
