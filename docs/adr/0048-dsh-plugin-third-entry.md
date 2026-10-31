# ADR-0048：DeepSeek Harness 插件作为第三种业务入口

- 状态：Accepted（2026-10-28，用户确认；作为 DSH 插件入口的实施与评审依据）；D5 于 2026-10-28 按 T0 实测修订（见 D5.1）
- 范围：在 Chrome 扩展与本机 Web 之外新增 DeepSeek Harness（下称 DSH）插件入口；不改变现有入口行为、数据 authority、端口分配与模块分发规则。

## 问题

1. 用户需要把空间、文件、Token 统计、投资四个模块放进 DSH，并分发给其他 DSH 用户；对方只装一个插件即可获得同等能力。
2. 现行规范只承认 Chrome Extension Page 与 ADR-0042 本机 Web 两个业务 Surface；模块随完整产品分发（ADR-0029／0031）。插件自带运行时属于新的入口与分发形态，必须先限定。
3. 四个模块的数据面不同：空间／文件属 Core，Token 统计经 `model-host:8766`，投资属 App Runtime。任何“复制一套业务”的做法都会产生第二个数据 writer。

## 决定

### D1 入口身份与拆分

DSH 插件是第三种业务 Surface。插件包只通过 DSH 插件机制安装／启用／卸载；不产生 `.app`、不注册 LaunchServices、不写 `/Applications`、不注册 Native Messaging Host、不设置登录启动项。产品身份仍是 Natives，插件不是第二个产品。

交付形态是**四个独立插件**，不是一个大插件加二级菜单：

- `@natives/dsh-space`、`@natives/dsh-files`、`@natives/dsh-tokenusage`、`@natives/dsh-fund`，各自注册 1 个 `sidebar.panellist` 条目和 1 个同名 `main` key；
- 每个插件可单独安装、单独启用、单独卸载，互不依赖 UI；
- 运行时（payload 解析、启动、护栏、URL 路由）以 `integrations/dsh/runtime/index.js` 为唯一源，**逐字节复制**进每个插件包（`runtime.js`），由 `dsh:check` 校验副本一致。原因是安装器对本地目录只做链接、不安装依赖，若声明共享 npm 包，别人单独装一个插件会解析失败；复制只影响代码分发方式，不产生第二个 Core、端口、payload 或数据 writer。

### D2 运行时同源

不 fork 运行时。插件按以下顺序解析：

1. 已在运行的 Core → attach，端口取状态目录锁记录；
2. 开发者本机的源码 checkout 构建（`target/debug|release/native-file-host`，仅当插件位于 checkout 内；同时指向同目录 `model-host`）→ 用其启动；
3. 本机已安装产品源（`NATIVES_DEV_PRODUCT_SOURCE`／系统源／`~/.natives-local/product-source`）→ 用其二进制启动；
4. 随包 payload（**已签名的产品源快照**：`product-manifest.json` 与 `product-manifest.sig`、`hosts/`（Core 与 app-runtime）、`model-host`、`ChromeExtension/`、`modules/`）→ 用其启动。

payload 不是散装二进制集合；Core 启动时按产品清单验签，模块首次握手再完成 per-user projection。各来源共用同一数据根与 `~/.natives/apps/<appId>`（debug Core 用 `~/.natives-local`，release Core 用 `~/.natives`）；仅隔离测试允许 `NATIVES_STATE_DIR` 覆盖。现有 Chrome／Web／Launcher 路径与启动方式不变。

### D3 单实例与端口

不新增固定监听端口。Core 仍用 8770；`NATIVES_WEB_PORT` 只用于隔离测试；attach 时读取锁文件端口。OS 锁保证同一时刻只有一个 Core：先起者胜，后到者 attach。禁止插件另起第二个 Core，或在同一库上双写。

### D4 认证边界

复用 ADR-0042／0045：一次性 Bootstrap → HttpOnly Cookie → CSRF → 精确 Origin。Bootstrap token 与 Cookie 只存在于宿主 Browser 标签的 `127.0.0.1` guest 中；不进入 DSH 页面上下文、不写日志、不持久化到插件配置。不新增免鉴权业务接口，不放宽 Origin 校验。

### D5 页面承载

页面承载以**主面板全屏 iframe**为默认（2026-10-28 修订，见 D5.2）。Core 只对携带签名 embed grant 的 HTML 文档响应输出 `Content-Security-Policy: frame-ancestors <ancestors>` 并省略 `X-Frame-Options`；其余响应继续 `SAMEORIGIN`。

插件**不得**自行创建 `<webview>`（主进程 `will-attach-webview` 会拒绝无租约的挂载）。iframe 只允许加载带 grant 的 URL。禁止：

- 用 DSH `webServer` 反向代理 Natives；
- 对无 grant 的响应改写或剥离 `X-Frame-Options`／CSP；
- 把 Natives 页面并入 DSH 同源（会继承 DSH 页面权限）。

主题通过 URL 参数映射：DSH dark → `?theme=volt`，light → `?theme=archive`；Core 页面启动时读取该参数并设置 `data-theme`，不写回用户偏好。语言仍用页面自身已保存的偏好，不伪造未提供的语言。

#### D5.2 签名嵌入例外（2026-10-28 用户确认）

1. `--launcher-url <target>` 增加可选 `--embed-ancestors <值>`：用 relay secret 做 HMAC-SHA256，生成 `exp.sig` 形式的短时 grant（≤120 秒），绑定“目标路由 + ancestors”。
2. ancestors 只允许 DSH 自有 scheme source（`dsh-app:` 与 `dsh:`）与回环
   `http://127.0.0.1:<port>`／`https://127.0.0.1:<port>`，由 Host 半生成
   （`dsh-app: dsh:` + `ctx.webServer.getOrigin()`），不接受浏览器端任意值。
   2026-10-29 实测修订：Desktop 窗口实际运行在 `dsh-app://app`
   （进程参数 `--standard-schemes=dsh-app`），`dsh:` 单独使用无法匹配，故
   scheme 白名单同时放行两者；仍不放宽到任意来源。
3. Core 校验 grant 后，仅对该 HTML 文档响应设置 `frame-ancestors`，并省略
   `X-Frame-Options`；grant 缺失、过期、目标或 ancestors 不匹配时维持 `SAMEORIGIN`。
4. 主面板用全屏 iframe 加载该 URL；不切换会话、不打开 Browser 标签。Browser 标签保留为回退，不再默认。
5. 该例外不放宽 Cookie／Origin／CSRF，也不允许非签名 URL 被任意来源嵌入。
6. 2026-10-29 实测修订：主面板 iframe 是跨站第三方上下文，`SameSite=Strict`
   会话 Cookie 永不携带、无 `Partitioned` 属性的 Cookie 被直接拒收，导致嵌入页
   无法完成 Bootstrap 交换（实测 `/web/session` 401、主题与启动均失败）。会话
   Cookie 改为 `SameSite=None; Secure; Partitioned; HttpOnly`（CHIPS，按
   top-level site 分区；`Secure` 在 loopback 可信源上可设置）。认证链不变：
   一次性 Bootstrap、CSRF、精确 Origin、Server 绑定 127.0.0.1 全部保留。
7. 2026-10-29 修订：嵌入文档**一律不显示自身产品侧栏**（左侧导航、品牌、工作区树、
   折叠按钮与分隔条）。判定不只看 `?standalone=1`：被框架承载
   （`window.top !== window.self`）同样进入 standalone，旧 Host 未带参数或参数被
   剥离时也不会退回带侧栏的形态。隐藏必须落在侧栏整列上，并把仍写死
   `grid-column:2` 的行（工具栏、状态栏、内容区）拉回整行——否则单列模板会留下
   一条空轨道，看起来仍是「多出来的一条左栏」。顶层打开（Chrome 扩展、本机 Web）
   行为不变，侧栏照常显示。
8. 2026-10-29 修订：嵌入文档改用**宿主色板**。`theme.css` 在 `html.standalone`
   下把基础角色重映射到 DSH 的 neutral-bluish 阶与单色强调（浅色 `#f9fafb`／`#0f1115`，
   深色 `#151517`／`#f9fafb`，边框、状态色同步取 DSH 语义值）；`archive`／`volt`
   两个主题值与 Archive 的蓝、Volt 的荧光绿在宿主外层保持不变。模块 iframe 以
   `&standalone=1` 继承同一套覆盖，模块 head 的内联脚本在首帧前打类。
9. 2026-10-29 修订：嵌入文档**不保留开合自身左侧栏的控件**——外壳页面的
   `#space-toggle-sidebar-btn`、`#app-stage-toggle-sidebar-btn`、`#apps-expand-sidebar-btn`
   一律隐藏；投资模块页头的 `#btn-toggle-left-sidebar` 折叠的是外壳 `app-sidebar`
   （经 `toggle-sidebar` 消息驱动），在宿主里没有对象，故隐藏并停止联动，外壳侧也
   忽略嵌入态下的该消息。右侧看板等模块自有控件保留。判定口径：外壳页面可用
   「`?standalone=1` 或被框架承载」，**模块页只能用 `?standalone=1`**——模块页永远在
   iframe 里，用 framed 判定会把 Chrome 扩展里的模块也误判为宿主嵌入。
7. 2026-10-29 实测修订（嵌入链第二段）：模块沙箱 UI 由 app-runtime 以
   `frame-ancestors <握手 origin><Web 端口 origin>` 限定，DSH 链路的完整祖先
   是 `dsh-app://app → http://127.0.0.1:<Web 端口> → 沙箱 UI`，故 app-runtime
   CSP 追加 `dsh-app: dsh:` 两个 scheme source（仅 DSH 注册这两个 scheme；
   握手 origin 与 Web 端口 origin 仍为精确校验）。

#### D5.1 Browser 标签时序（已被 D5.2 取代，仅回退时适用）

`ctx.sidebarRight.openTab` 绑定「屏幕上的会话」；全局面板占用主栏时 `mounted` 为
`undefined`，需要先 `selectPanel(null)` 再打开。该时序只在回退到 Browser 标签时
使用；默认形态已改为 D5.2 的主面板 iframe，不再切换会话。


### D6 分发与签名

主渠道为 npm bundle 加市场索引，安装走 DSH `install_bundle`。payload 携带版本与逐文件 sha256 清单，启动前校验，失败即拒绝启动。

npm 渠道不要求 Developer ID 或公证：macOS arm64 由链接器写入 ad-hoc 签名即可执行，程序化安装不产生 quarantine。若改为浏览器下载的 zip／dmg 分发，必须补 Developer ID 签名与公证后才可宣称可用。

### D7 版本护栏

启动前比较 `~/.natives/product/activation.json` 记录的各模块 `dataSchemaVersion`（及产品清单版本）与所选运行时的清单：

- 记录更高 → 拒绝启动，提示升级插件，不得降级写；
- 记录更低或缺失 → 走现有迁移；
- 文档与提示统一为“先升插件，再升应用”。

### D8 非目标

- 不恢复通用 Plugin Runtime、Agent、Jobs 或第二数据库权威；
- 不允许模块独立分发（ADR-0029／0031 不变）；
- 不把模块业务数据自动送入模型上下文；工具化访问须另立 ADR 并限定范围；
- 不新增常驻守护进程；Core 仍按 ADR-0042／0047 在最后一个 owner 释放后回收。

## 影响

- Core 增量：
  - `--launcher-url <target>`：与 `--launcher-web` 共用路由白名单和 `native:bootstrap`，只输出 URL 不打开浏览器；`fund` 加入白名单。
  - 页面启动支持 `?theme=archive|volt`：只覆盖本次渲染，不写回用户偏好。
  - `--launcher-web`、服务模式、relay 与现有测试不变。
- 插件：四个独立 bundle，各自携带由同一源生成的 `runtime.js`；每个 bundle 只有 1 个入口、1 个主面板壳，无二级菜单。
- 规范：R-T1 的 Surface 定义、R-T8 第三入口边界、端口登记表、README 任务索引。
- 验收新增：四个插件可独立安装／启停／卸载；runtime 副本逐字节一致且只共用一个 Core／一份 payload；双形态同时开启、数据一致；旧运行时打开新数据被拒绝；无 Natives 机器上四模块可用。
- 风险：`ctx.sidebarRight.openTab` 与 Browser 租约行为需首日验证；DSH rc 升级需回归。

## 验收

1. 无 Natives 的干净 macOS 用户分别安装四个插件后，四个入口各自打开对应的宿 Browser 标签，功能与 Chrome 入口一致；任一插件可单独卸载而不影响其余。
2. 四个插件共用一份 runtime 与一份数据；不得出现第二份 payload 或第二份库。
3. 已有 Natives 数据与运行实例时，插件 attach 同一 Core，读到的数据与 Chrome 页面一致；两边同时操作不产生双写或丢更新。
4. 关闭模块标签后 Surface 释放，Core 按原时限回收；Chrome 页面仍开时 Core 不退出。
5. 数据 schema 高于所选运行时 → 明确拒绝并提示，不写入；恢复一致版本后可正常启动。
6. 主题（Archive／Volt）随 DSH 明暗切换；插件主面板与标签页均无未翻译硬编码；窄窗与键盘可用。
7. 卸载插件不删除用户数据；重装可继续使用。
8. 现有 Chrome／Web／Launcher 全部门禁与验收结论不回退。
