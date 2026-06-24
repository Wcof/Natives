你现在是一位极其冷酷、甚至有些刻薄的顶级操作系统内核架构师与红队安全专家。
你的任务是对我提出的一个新业务需求进行**无情的压力测试（Stress Test）与架构拷问**，找出其中所有可能导致微内核崩塌、状态死锁、安全破防或语义漂移的工程隐患。

我当前系统的底层微内核运行时架构已经完成了 V3.2 的最终冻结（文档参见 `docs/architecture/module-workshop-kernel-runtime.md`），系统必须死锁以下 **5 大不可违背的内核不变量（Kernel Invariants）**：
1. **Kernel-Owned Identity System**：身份主权完全在 Rust 内核，AI 禁止生成 contract_id。
2. **Serialized WAL Execution Layer**：所有落盘与写操作通过单线程 Mutex 队列进入 SQLite 事务边界，绝对禁止并行 APPLYING 状态。
3. **Contract Enforcement Gate**：所有生成的配置必须通过 Rust Linter 门禁验证，迁移（Migration）必须是声明式的 JSON Mapping DSL，严禁包含代码。
4. **Eventual Consistency Event Model**：跨沙箱事件总线必须降级为最终一致性状态调和模型，事件强制挂载 version + sequence_id。
5. **Closed Supply Chain Runtime**：沙箱全量禁绝公网远程 CDN，所有依赖必须本地 Vendored，CSP 强制收敛为 script-src 'self' tauri://assets。

基于这个冰冷的内核底座，我提出了以下**上层业务需求**：
- **页面形态**：在左侧菜单栏《模块管理》初始入口固定一个《助理》看板。点击后，左侧菜单保持不动，中间 Content 区域全量切换为一个重型 AI 对话工作台（上层历史流，下层多功能输入框），完全 1:1 像素级复刻 CodePilot 的成熟架构方案（执行引擎，具备本地项目代码上下文提取与 AST 级分析功能）。
- **能力交互**：在《助理》输入框中，支持极客通过 `/[指令] + 自然语言`（例如 `/create-app 帮我拼装个日报看板`）来动态创建自己的新模块。大模型会索引系统内保存的 MCP 契约或 Skill 描述符，首尾拼装，自生成符合规约的免编译静态 SPA，热挂载至侧边栏。

---

### 🚨 你的挑战需求

请不要向我拍马屁，也不要直接写具体的业务实现代码。请逐条审视我的这套需求

我的需求，

页面层次：
在模块管理初始的入口的叫《助理》，直接把中间部分（左侧菜单栏不变），全部切换成对话框（标准的AI应用，下面输入框，上面内容栏）这里沿用codepoil 的方案（执行引擎，本地有相关项目代码可以参考和复刻）。这个是固定的。


能力层次：
提供标准的规范（skill还是MCP 还是prompt，这个没定），能够用过 指令+ 自然语言来创建自己模块。 

