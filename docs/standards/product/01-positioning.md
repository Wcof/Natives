# 产品 01 · 定位与边界

> 版本：4.0.0 · 日期：2026-09-14
> 依据：ADR-0020、ADR-0021、ADR-0027、ADR-0029、ADR-0030

#### R-P0 · 产品身份唯一

- **等级**：MUST
- **规则**：Natives 是 AI Native Personal Workspace，是用户安装、打开和更新的唯一产品。
- **禁止**：把 Host、内置模块、AI 工具或浏览器扩展描述成另一个独立产品。

#### R-P1 · 一级信息架构固定

- **等级**：MUST
- **规则**：一级入口为 Home、Files、Apps、AI、Data & Usage、Settings。
- Home 承载多个 Workspace；Apps 是内置模块入口与偏好管理；完整 Usage 进入 Data & Usage。
- 新入口必须归入上述领域；改变一级 IA 需要 ADR。

#### R-P2 · Home 是可配置 Workspace

- **等级**：MUST
- **规则**：Home 支持多 Workspace 和 `structured | free` 两种有界布局。Widget 是随 Extension 构建的普通 JavaScript renderer、配置和版本化布局。
- **禁止**：Widget Marketplace、运行时插件加载、远程代码、Worker 平台、无限画布或第二数据总线。

#### R-P3 · Widget 只做轻量投影

- **等级**：MUST
- **规则**：Widget 通过现有领域 client 查询 Files、Apps、AI、Usage 等真实数据；相同查询和时钟在页面级去重。
- **禁止**：Widget 直接访问 SQLite、扫描工具日志、读取 Secret、查询进程、调用 Provider 或创建永久独占轮询。

#### R-P4 · 内置模块属于完整产品

- **等级**：MUST
- **规则**：基金等官方功能随完整 Natives 安装包交付。应用中心只提供打开、显示/隐藏、偏好和数据管理。
- 首次打开只做当前用户的数据初始化或迁移；模块不独立下载、安装、更新、卸载、发布或进入在线 Catalog。
- 新增模块只能通过新的完整 Natives 版本。

#### R-P5 · 领域边界稳定

- **等级**：MUST
- Files 拥有文件 CRUD、Trash、Watch、Search 和预览。
- Apps 拥有内置模块登记、偏好、运行与 Surface；模块业务库由模块自己拥有。
- Model 拥有 Provider、Connection、Credential、Local Proxy、AI Tool Integration 和 Usage。
- Provider 是厂商；Connection 是 endpoint/protocol；Credential 是 Keychain 引用。
- Usage/Cost 必须使用真实事件、账单或可追溯价格版本，估算值明确标注。

#### R-P6 · 只允许冻结的进程边界

- **等级**：MUST
- **规则**：生产进程只有按需 Files Host、单用途 Model Host、按需 Built-in App Runtime Process（统一使用 `natives-app-runtime` 二进制，每个打开的模块一个按需进程实例）和短命 Launcher。
- 额外本机进程或 Surface 必须先有 ADR；不得恢复通用 Daemon、Agent Runtime、Jobs、Capabilities、Harness、Workshop 或 Plugin Runtime。

#### R-P7 · 一个状态只有一个权威

- **等级**：MUST
- **规则**：实现顺序为复用现有 authority → 增加窄 adapter → 在 owner 内修改。禁止双写、双执行、复制数据库或长期 fallback。

#### R-P8 · 能力状态必须诚实

- **等级**：MUST
- **规则**：能力只声明 `implemented | partial | unsupported | unavailable`。用户可见状态必须有真实 source、调用链和验证证据。
- 错误不得转为空数组、零值或成功提示。

## 合规自检

- [ ] 新功能属于固定 IA。
- [ ] 内置模块没有独立安装和发布语义。
- [ ] Widget 没有成为 backend 或运行时平台。
- [ ] 进程边界只包含冻结 Host。
- [ ] 状态与数据只有一个 owner。
- [ ] 页面没有用假数据掩盖缺失能力。
