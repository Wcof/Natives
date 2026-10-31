# DeepSeek Harness 插件接入 · 执行提示词

> 依据：[ADR-0048](../adr/0048-dsh-plugin-third-entry.md)、[技术 01](../standards/technical/01-layering.md)、[端口登记表](../standards/technical/05-port-registry.md)
> 状态：待执行（2026-10-28，形态 A：四个独立插件 + 宿主 Browser 标签）

## 0. 任务

在 Natives 仓库实现 ADR-0048 的形态 A：交付**四个独立**的 DeepSeek Harness（下称 DSH）插件，分别是空间、文件、Token 统计、投资。每个插件只有一个左侧入口、一个主面板壳，点击后由 DSH 宿主在右侧 Browser 标签中打开对应模块页面。四个插件共用一套运行时、一份 `~/.natives` 数据；现有 Chrome 扩展、本机 Web、Launcher 三种入口的行为与验收结论不得回退。

不能做成“一个大插件 + 启动页 + 二级菜单”。四个插件必须能单独安装、单独启用、单独卸载、互不依赖 UI。

## 1. 完成定义（全部满足才算完成）

1. 四个独立 bundle：`@natives/dsh-space`、`@natives/dsh-files`、`@natives/dsh-tokenusage`、`@natives/dsh-fund`，各自注册 1 个 `sidebar.panellist` 条目和 1 个同名 `main` key。
2. 点击任一条目，DSH 右侧 Browser 标签打开对应页面：`space.html`、`files.html`、`app.html?app=tokenusage`、`app.html?app=fund`，显示真实数据。
3. 四个插件不出现二级菜单、启动页或“先选模块再打开”。
4. 四个插件各自携带逐字节相同的 runtime 副本（单一源生成）；不得出现第二份 payload、第二份数据库、第二个 Core。
5. 已有 Natives 数据时直接复用；没有数据时首次初始化。
6. 同一时刻只有一个 Core；插件与 Chrome 页面可同时打开、数据一致。
7. 关闭模块标签后 Surface 释放，Core 按现有 60 秒规则回收；Chrome 页面仍开时 Core 不退出。
8. 数据 schema 高于所选运行时 → 明确拒绝并提示升级插件，不写入。
9. 主题随 DSH 明暗切换：打开 URL 带 `?theme=archive|volt`，Core 页面按参数渲染且不写回用户偏好。
10. 任一插件单独卸载不影响其余三个；卸载不删用户数据，重装可用。
11. 门禁全部通过：`cargo fmt --check`、受影响 package 的 `cargo test`、`standards:check`、`extension:check`、`perf:check`、`git diff --check`。
12. 报告区分“已实现且已验证／已实现未验证／未实现／阻塞”，不得用安装成功代替功能验证。

## 2. 不可违反的约束

- **四个独立插件**：不合并成一个入口，不做二级菜单；单个插件缺失时其余三个照常工作。
- **只做增量**：不改 `--launcher-web` 现有行为、不改扩展与 Web 启动链、不改数据 owner。
- **双形态共存**：插件是第三入口，不是替代；不产生 `.app`、Dock、LaunchServices、Native Messaging 注册、登录启动项、安装器。
- **单实例单数据**：不新增固定端口；不双写；不把插件数据根指到别处（隔离测试除外）。
- **页面只走宿主 Browser 标签**：插件不得自行创建 `<webview>` 或 iframe（主进程会拒绝无租约挂载）；不反向代理 Natives、不剥离 `X-Frame-Options`／CSP、不并入 DSH 同源。
- **不把业务数据送进模型上下文**：本任务只做 UI 与生命周期；工具化访问须另立 ADR。
- **不改用户数据**：本地验证使用隔离 `NATIVES_STATE_DIR`；真实库只读核对。
- **不提交、不推送、不发布、不安装到真实用户环境**，除非用户另行明确授权。

## 3. 已核实事实（直接使用，不要重新猜测）

| 事实 | 位置 |
|---|---|
| `--launcher-web <space\|files\|tokenusage>` 已存在：`ensure_service` → `native:bootstrap` → 拼 `#token=` URL → 打开 Chrome | `crates/native-file-host/src/main.rs:64`、`src/service/launcher.rs:6,19` |
| 一次性 token 为 43 字符，放在 URL fragment | `src/service/launcher.rs:31` |
| 端口：默认 8770，`NATIVES_WEB_PORT` 可覆盖；锁文件记录端口 | `src/service/mod.rs:40,107` |
| attach：`ensure_service` 先探 health，不健康才启动 | `src/service/mod.rs:196` |
| 数据／资产 env：`NATIVES_STATE_DIR`、`NATIVES_APPS_ROOT` | `src/service/mod.rs:53,169` |
| 产品源：`NATIVES_DEV_PRODUCT_SOURCE` 显式 override 在两种 profile 都优先；布局为 `model-host`、`hosts/natives-app-runtime`、`ChromeExtension/` | `src/app_product.rs`、`src/service/static_files.rs:7` |
| 产品清单与签名：根目录 `product-manifest.json` + `product-manifest.sig`；schemaVersion 2，platform／arch 必须匹配 | `src/app_store/product.rs:19,145-175` |
| 首次 `apps:handshake` 执行一次性 per-user projection（`configure_product`） | `src/app_dispatch.rs:36-57` |
| `product_activation_path(apps_root)` = `<apps_root 的父目录>/product/activation.json` | `src/app_activation.rs:103` |
| Core 自己拉起 model-host 与 app-runtime | `src/service/model_worker.rs:24`、`src/service/app_runtime.rs:286` |
| 现有页面：`space.html`、`files.html`、`app.html`、`apps.html` | `extension/`、`src/service/static_files.rs:95` |
| 四个模块入口：`space.html`、`files.html`、`app.html?app=tokenusage`、`app.html?app=fund` | `modules/{fund,tokenusage}/module.json:12` |
| 页面主题来自 `natives-theme` 存储（archive／volt），当前**不读** URL 参数 | `extension/space.js:294-301`、`files.js:78-84`、`app.js:18-23` |
| 模块 iframe 已用 `?theme=` 传主题（说明该参数是可接受的现有模式） | `extension/app.js:461` |
| 页面响应头 `X-Frame-Options: SAMEORIGIN` | `src/service/server.rs:311` |
| DSH 主窗口 `webviewTag` 开启，但主进程 `will-attach-webview` 只放行带租约的 `about:blank#<leaseId>`；插件自建 webview 必被拒绝 | 安装包 `app.asar`：`DesktopBrowserGuests.bind` |
| 官方页面能力：`ctx.sidebarRight.openTab('browser', { params: { url } })`；Browser 允许 loopback、排除 DSH 自身端口 | `@deepseek-ai/dsh-client-ui-sidebar-browser` README |
| DSH：`sidebar.panellist` 的 id 与 `main` 的 key 同名配对；缺 `main` 会抛错且不切换；图标为注册方组件，owner props 为 `size`、`active` | DSH `Slots.listSubTree`、`ui-sidebar` README |
| DSH：插件包由 `package.json` 的 `dsh.bundle.patch` + `dsh.client` 组成；安装走 `plugin_manager install_bundle` | `cordis-plugin-development` skill |

## 3.1 开工前判定（不满足即停）

1. 可用于分发的 payload 必须是**生产签名的 sealed 产品源**。只有散装二进制或开发签名源时，T6 的“干净机器”验收直接阻塞：不得用散装件拼 payload，也不得用开发签名冒充生产签名。
2. 本机已有产品源时可走 `system` 分支；但 T6 必须在**没有产品源**的隔离环境验证 payload 分支。
3. `ctx.sidebarRight.openTab` 与 Browser 租约行为以 T0 实测为准；若不可用，停，不要改用自建 webview／iframe。
4. `install_bundle` 若因审批不可用，T0 即停，不绕过。

## 4. 目录与产物

```text
integrations/dsh/
├─ runtime/                       # 运行时唯一源（不单独安装）
│  ├─ package.json
│  └─ index.js                    # 解析、启动、护栏、URL 构造
├─ space/                         # @natives/dsh-space
│  ├─ package.json                # dsh.bundle.patch + dsh.client
│  ├─ cordis.patch.yml
│  ├─ index.js                    # Host：import './runtime.js'
│  ├─ runtime.js                  # 由 scripts/dsh-sync-runtime.mjs 生成
│  ├─ client.js                   # 1 入口 + 1 主面板壳
│  ├─ locale/{zh,en}.json
│  └─ icon.svg
├─ files/  tokenusage/  fund/     # 同构
scripts/dsh-sync-runtime.mjs      # 源 → 四个副本，--check 校验漂移
scripts/dsh-payload.mjs           # sealed 源 → payload + 校验清单
```

payload 是**已签名的产品源快照**，形状必须与 `default_system_source()` 一致（实际文件名是根目录的 `product-manifest.json`／`.sig`，不是 `product/manifest.json`）：

```text
payload/
├─ hosts/native-file-host
├─ hosts/natives-app-runtime
├─ model-host
├─ ChromeExtension/
├─ modules/
├─ product-manifest.json
└─ product-manifest.sig
```

插件自己的 `payload/manifest.json` 另记 `version` 与逐文件 `sha256`，用于解包完整性与升级；不替代产品签名。

## 5. 任务分解

### T0 · 可行性闸门（先做，未通过不得进入 T4）

1. 建一个最小插件：Host 空 `apply()`；Client 注册 `sidebar.panellist` 的 `natives-probe` 与 `main` 的 `natives-probe`，面板内一个按钮调用 `ctx.sidebarRight.openTab('browser', { params: { url: 'http://127.0.0.1:8770/space.html' } })`（Core 未起时允许失败，先验通道）。
2. `plugin_manager install_bundle` 安装；`cordis_inspect_query`（`Slots.listSubTree`，root 分别为 `sidebar.panellist` 与 `main`）确认注册。
3. 验证：入口可见；点击切到面板；切走卸载；调用 `openTab` 能创建右侧 Browser 标签并尝试加载 loopback 地址（无浏览器控制时请用户目视确认并记录）。
4. `main` 中除 `conversation` 外的 key 没有 Session 绑定：不要依赖 session props 或 `commands.execute`（需要 Agent）。
5. 任一项不成立 → 停，报告；**不得**改为自建 webview、iframe 或反向代理。

验收：四项逐条“成立／不成立＋证据”；记录 `openTab` 的实际调用形态与标签行为。

### T1 · Core 增量：`--launcher-url`、`fund` 路由、`?theme=`

1. 把 `launcher::open_web` 拆成“准备 URL”和“打开浏览器”两段，`open_web` 行为不变。
2. 新增 `--launcher-url <target>`：输出单行 JSON `{"url":"..."}`，不打开浏览器；错误退出码与 `--launcher-web` 一致。
3. `route()` 白名单新增 `fund` → `app.html?app=fund`；非法输入仍拒绝。
4. 页面主题参数：`space.html`、`files.html`、`app.html`、`apps.html` 启动时读取 `?theme=archive|volt`，合法则覆盖本次 `data-theme`；**不写回** `natives-theme`。非法值回退已保存偏好。
5. 测试：白名单（fund／非法输入）；URL 含 `#token=`；`open_web` 仍走打开分支（注入 opener，不在测试中启动 Chrome）；主题参数合法／非法／缺省三种。
6. 不新增端口、不改默认端口逻辑。

验收：`cargo test -p native-file-host` 通过；手工：`--launcher-url space` 在 Core 未起时启动并返回 URL，已起时 attach；带 `?theme=volt` 打开为深色且刷新后偏好未变。

### T2 · payload 打包器（sealed 产品源）

1. `scripts/dsh-payload.mjs`：输入为**已签名的 release 产品源**，输出 `payload/` 与校验清单；不得用散装 debug 二进制拼装。
2. 原样保留 `product-manifest.json`、`product-manifest.sig`、`hosts/`、`model-host`、`ChromeExtension/`、`modules/` 的路径与字节；只整体复制，不重排、不改名、不重签。
3. 缺项、`platform`／`arch` 不匹配、签名校验失败 → 立即失败，不生成半成品。
4. 重复运行稳定；打印体积与文件数。

验收：`node scripts/dsh-payload.mjs --check` 全量校验；篡改任一文件后失败；Core 能验签。

### T3 · runtime 包：解析链、产品激活、版本护栏、URL 服务

1. 解析顺序：attach（health + 锁端口）→ 已安装产品源 → 随包 sealed payload；日志不含 token。
2. env 只设 `NATIVES_DEV_PRODUCT_SOURCE`（显式 override），隔离测试再加 `NATIVES_STATE_DIR`；**只传子进程，不改全局 `process.env`**；不要设 `NATIVES_MODEL_HOST_BIN`／`NATIVES_APP_RUNTIME_BIN`／`NATIVES_WEB_ROOT`。
3. 产品激活：首次 `apps:handshake` 由 Core 完成 `configure_product`；验收用 `apps:product_status` 确认 `integrityValid`／`current`／`configured`，不手写 `product/activation.json`。
4. 版本护栏：读 `<apps_root 父目录>/product/activation.json` 的 `dataSchemaVersion` 与所选源比较；记录更高 → 拒绝。
5. URL 服务：runtime 提供 Client 可调用的 Host 通道，优先 `webServer.register` 同源只读 `GET`（返回 `{url}`，不代理解析、不转发业务数据、不缓存；只允许同源）。**不要**用 `commands.execute`。路由接受 `{ target, theme }`，把 `theme` 作为查询参数拼进返回 URL（fragment 里的 token 保持原位）。
6. 四个插件各自携带逐字节相同的 `runtime.js` 副本（源在 `runtime/index.js`）；共享同一份 payload、同一个 Core。不得重复启动 Core，也不得出现第二份数据库。
7. 单元测试：护栏三态、解析顺序、缺 payload、路由错误码、theme 拼接、token 不出现在日志、副本一致性。

验收：单元测试通过；隔离状态下 `space`／`files` 可打开，`fund`／`tokenusage` 能激活并启动 Runtime；本机有数据时 URL 打开看到旧数据。

### T4 · 四个 Client 插件

1. 每个插件注册自己的 `sidebar.panellist` id（`natives-space` 等，不复用 `plugins`／`schedules`／`conversation`）与同名 `main` key；`label` 走 locale；图标自绘 SVG 组件，响应 `active`／`size`。
2. 主面板壳：模块标题、当前状态（未连接／启动中／已打开／版本拒绝）、`打开模块` 与 `重新打开` 按钮；**不承载业务页面**。
3. 选中入口时自动打开一次 Browser 标签；用模块内状态去重，避免每次切换都新开标签；标签被关后可手动重开。
4. 状态与错误不吞：Core 未就绪、令牌失败、版本拒绝、`openTab` 失败都要可读文案。
5. 主题：从 DSH 读取明暗，构造 URL 时带 `?theme=archive|volt`；DSH 切换后新开标签生效，已开标签不强制刷新。
6. 卸载：effect cleanup 清理监听器与定时器；不残留自动打开逻辑。
7. 窄窗与键盘：入口可 Tab 到达、回车切换；面板不劫持 DSH 快捷键。

验收：四个入口各自打开正确页面；无二级菜单；单独卸载一个不影响其余；主题参数正确；中英文标题齐全。

### T5 · 生命周期与共存

1. 关闭 Browser 标签 → Surface 释放；Chrome 页面仍开时 Core 不退出，全关后 60 秒回收（ADR-0047 不变）。
2. 进程核对：`pgrep -af native-file-host` 中 `--service` 实例任一时刻至多 1 个（relay 不算；测试环境除外）。
3. 四个插件同时启用 → 仍只有一个 Core、一份 payload；启动顺序不同结果一致。
4. 两边同时写（如新建空间）→ 另一端刷新可见；无覆盖、无报错。
5. 重启 DSH → 插件重新 attach，不重复初始化。

验收：进程、数据、回收三组证据；失败保留原始输出。

### T6 · 分发与升级

1. `npm pack` 生成四个包（runtime 源不单独分发）；隔离 profile 安装（若只接受目录，解包到目录再装并记录方式）。
2. 先只装 `space` 验证“装一个能用一个”；再装其余三个验证独立性与 runtime 副本一致。
3. 打包核对：`payload/`、`locale/`、`icon`、`cordis.patch.yml` 在 `files` 中；无开发夹具、无绝对路径。
4. 卸载：卸载单个插件不影响其余；全卸后数据仍在；重装可用。
5. 干净状态验收：不设系统产品源、`NATIVES_STATE_DIR` 指向空目录、`runtime: bundled`，确认四个入口可用、`apps:product_status` 为 `integrityValid`／`configured`、`fund`／`tokenusage` 能启动 Runtime。

验收：干净 profile 安装成功；装一个／装四个两种状态都验证；卸载／重装往返；记录 tarball 大小与文件数。

### T7 · 最终门禁与文档

1. `rtk env -u CARGO_TARGET_DIR cargo fmt --check`
2. `rtk env -u CARGO_TARGET_DIR cargo test -p native-file-host`
3. `rtk npm run standards:check`
4. `rtk npm run extension:check`
5. `rtk npm run perf:check`
6. `rtk git diff --check`
7. 更新 ROADMAP（完成项与未验证项分开写）；如实现偏离 ADR，先改 ADR 再改代码。

## 6. 停止条件（遇到即停，不自行绕过）

- `ctx.sidebarRight.openTab` 不存在或 Browser 标签拒绝 loopback → 停，报告，不改用自建 webview／iframe。
- 需要修改现有入口行为、`X-Frame-Options`、CSP、Origin 校验 → 停，先修订 ADR。
- 版本护栏无法从 `product/activation.json` 得到可靠比较 → 停，提出替代方案，不猜字段。
- `install_bundle` 需要审批而当前会话无法批准 → 停，说明阻塞点。
- 四个插件无法共享一份 runtime 或会重复启动 Core → 停，先解决共享模型。
- 任何一步需要写真实用户数据或删除数据 → 停。
- 现有门禁出现与本次改动无关的失败 → 记录原始事实，不修改无关代码掩盖。

## 7. 交付报告格式

```text
## 结果
- 已实现且已验证：
- 已实现未验证：
- 未实现：
- 阻塞：

## 证据
- 安装结果：application / warnings（逐插件）
- Slot：四个 sidebar.panellist 与四个 main 的实际注册
- 打开：每个模块的 URL、Browser 标签标题、数据来源（真实/隔离）
- 独立性：单独卸载一个后其余三个的行为
- 进程：`--service` 实例数与回收时间
- 门禁：逐条命令与结果

## 偏离
- 与 ADR-0048 的差异（无则写“无”）
```
