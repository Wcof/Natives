# 产品 02 · 功能状态与发布门槛

> 版本：4.0.0 · 日期：2026-09-14

#### R-F1 · 状态必须可验证

- **等级**：MUST
- `implemented` 必须具备真实 source、生产调用链和测试证据。
- `partial` 必须说明缺口；`unsupported` 必须说明边界；`unavailable` 必须说明当前失败原因。

#### R-F2 · 用户可见数据必须真实

- **等级**：MUST
- 文件、应用、Provider、quota、usage、cost、进程、端口和工具检测结果必须来自领域 query。
- 无来源显示 Unknown/Unavailable；估算显示“约”或“估算”，并携带价格/时间口径。

#### R-F3 · 四种 UI 状态分离

- **等级**：MUST
- 每个异步视图区分 loading、empty、error、unsupported。旧数据可在刷新失败时保留，但必须显示 stale/error。

## 目标功能矩阵

| 域 | 当前目标 | 禁止扩展 |
|---|---|---|
| Home | 多 Workspace、Structured/Free、Widget 目录、布局与外观 | 运行时插件、无限画布 |
| Files | CRUD、Trash、Watch、Search、Recent/Favorite、预览与编辑 | 第二文件 authority |
| Apps | 内置模块打开、显示隐藏、偏好、数据管理、故障恢复 | 模块下载/安装/更新/卸载、在线商店 |
| AI | 内置 AI 连接（地址／协议／模型／默认模型／Keychain 引用）、直连供应商推理、Tokenusage | 模型代理／路由、访问密钥、账号池与轮换、代理内核、第三方工具配置注入 |
| Data & Usage | 多 AI 工具 Token、成本、会话、账单、预算与提醒 | 假 quota、第二 Event Platform |
| Settings | 通用、外观、AI 连接、用量记录、额度与凭据、后台常驻和安装诊断 | 重复业务页面、模型代理／路由管理 |
| Launcher | 打开 Chrome、扩展安装引导、目录定位、简短诊断 | 第二套业务 UI、后台常驻 |

#### R-F4 · AI 与 Secret Gate

- **等级**：MUST
- 直连推理上线前覆盖协议 fixture、stream/取消/EOF/断连、缺 usage、缺价格、quota credential 只读刷新和 Keychain locked/rollback。
- 不使用模型代理内核、访问密钥、别名路由、账号池或自动重试；页面与模块只接收 `providerId`+`modelId` 安全引用。
- Secret 不得进入页面、磁盘明文、日志或错误详情。

#### R-F5 · 第三方工具配置退役

- **等级**：MUST
- Natives 不再写入第三方 AI 工具配置，也不恢复旧代理地址、旧备份或旧访问密钥。
- 旧代理配置必须显示明确退役说明；用户自行修改的外部文件不得被静默覆盖或自动删除。

#### R-F6 · Home 与生命周期 Gate

- **等级**：MUST
- 相同 query/timer 不随 Widget 数量线性增长；drag/resize move 写库为 0；hidden 工作暂停。
- 20 Widget、100 次增删/布局循环和 30 分钟真实 Chrome soak 必须满足性能标准。

#### R-F7 · 完整产品 Gate

- **等级**：MUST
- 候选必须包含 Launcher、Extension、主 Host 和发布清单声明的真实内置模块。
- 断网首次打开只初始化数据；重装、修复、升级保留偏好、用户数据和 Keychain；隐式降级拒绝。
- 本地开发验收与正式签名/公证/发布验收必须分列。

## 发布前死亡证明

- 旧工作台、Daemon、Agent、Jobs、Capabilities、Workshop、Plugin Runtime 无生产引用。
- 模型代理／路由业务无生产引用：无本地推理代理监听、访问密钥、别名路由、账号池、代理内核、OAuth 登录或第三方工具配置注入；`third_party/cliproxyapi` 及 SDK／补丁不再进入构建或安装载荷。
- 旧模块发现、传输、独立打包、独立可执行文件（如 `fund-host`）、独立 Native Messaging 注册（如 `com.natives.app.a<hash>`）和用户目录 `runtime/<version>/app` 载荷无生产引用。
- 产品内置应用使用统一 `natives-app-runtime` 二进制，无独立模块可执行文件。
- 无孤儿进程、端口、Watcher、Timer、Listener、Native Port 或任务；模块页面关闭或断开后 ≤2 秒进程完全退出。
- 当前 Standards、架构检查、Extension、Rust、Go、性能和完整安装路径全部通过。
