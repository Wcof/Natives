# DSH 插件移植收口 · 证据（2026-10-29）

对应 Goal：完整移植为 DeepSeek Harness 桌面端插件（分发／独立运行／Token 统计页重做／样式统一／端到端检查）。
前序：[dsh-plugin-t0-2026-10-28](dsh-plugin-t0-2026-10-28.md)、[交接上下文](../dsh-plugin-handoff-2026-10-28.md)、[ADR-0048](../../adr/0048-dsh-plugin-third-entry.md)。

## 1. 交付物

- 四个自包含 npm tarball（含 README）：`integrations/dsh/dist/@natives-dsh-{space,files,tokenusage,fund}-0.0.1.tgz`。
- `scripts/dsh-payload.mjs`：从完整签名产品源密封四份 `payload/`（manifest+sig、hosts/、model-host、ChromeExtension/、modules/ui），生成 `payload-manifest.json` 逐文件 sha256 清单；运行时 `verifyPayload` 启动前校验（ADR-0048 D6），损坏／篡改／路径逃逸一律拒绝启动。分发 README（总册 + 每插件）写明安装／验证清单与签名边界。
- `.gitignore`：`integrations/dsh/*/payload/`、`integrations/dsh/dist/`。

## 2. 修复的缺陷（全部先复现后修复）

1. **嵌入 URL 畸形查询（Core）**：`prepare_url_embedded` 对 `app.html?app=fund` 直接拼 `?embed=`，产生 `?app=fund?embed=…`，`embed::query_value` 解析不到 grant → fund/tokenusage 嵌入必被 XFO 拦截。改为查询合并（`merge_embed_query`）并统一追加 `standalone=1`；新增 grant 闭环测试。
2. **父来源 scheme 实测修正（Core + runtime + ADR）**：DSH Desktop 页面实际运行在 `dsh-app://app`（进程参数 `--standard-schemes=dsh-app`），`frame-ancestors dsh:` 不匹配 → 主区白屏。`embed::valid_ancestor` 放行 `dsh-app:`/`dsh:` 两个 scheme source，runtime 改铸 `dsh-app: dsh: <webServer origin>`；ADR-0048 D5.2 已补记。
3. **运行时 `URLSearchParams` 重序列化**：`?theme=` 经 URLSearchParams 回写把 mint 的 `%20` 变 `+`，Core 侧 `+` 不按空格解码 → grant 校验失败。改为手工拼接 `theme=`（保留原编码），Core `percent_decode` 兼容 `+`→空格；两侧均有测试。
4. **第三方 iframe 会话 Cookie（Core + ADR）**：主面板 iframe 是跨站第三方上下文，`SameSite=Strict` 永不携带、无 `Partitioned` 的 Cookie 被拒收 → `/web/session` 401 → `appOpenFailed`、主题不生效。会话 Cookie 改为 `SameSite=None; Secure; Partitioned; HttpOnly`（CHIPS；loopback 可信源可设 Secure）；认证链（Bootstrap/CSRF/Origin）不变，ADR-0048 D5.2 已补记。
5. **模块沙箱嵌入链（app-runtime + ADR）**：模块 UI 的 `frame-ancestors` 只含握手 origin 与 Web 端口 origin，DSH 链路顶层 `dsh-app://app` 被拒。`runtime_csp` 追加 `dsh-app: dsh:` 两个 scheme source（握手 origin 与 Web 端口仍精确校验），ADR-0048 D5.2 已补记。
6. **standalone 选择器（CSS）**：`#app-sidebar` 只有 id 无 class（`.app-sidebar` 匹配不到），files.css 的 `:not(.space-page)` 误写在 html 上。两处修正，真实 Chrome 断言通过。

## 3. 独立运行模式（目标第 2 条）

`?standalone=1`（仅由 `prepare_url_embedded` 注入）→ `platform/standalone-boot.js` 首帧前打 `html.standalone`，页面 JS 跳过应用菜单挂载，CSS 隐藏产品级聚合导航：

- `app.html`：隐藏 `.app-shell-nav`、`.app-shell-head`，只留模块舞台。
- `space.html`：侧栏只留工作区树，隐藏品牌／文件搜索／文档快捷方式／应用菜单／设置。
- `files.html`：隐藏「个人空间」链接／应用菜单／设置，保留文件自身目录导航、搜索与折叠按钮。

真实 Chrome 断言 13/13 通过（含对照：非 standalone 不受影响）：`.tmp/dsh-goal/verify-standalone.mjs`。新测试 `extension/standalone-boot.test.mjs` 并入 `extension:check`。

## 4. Token 统计页重做（目标第 3 条）

`modules/tokenusage/ui/src` 全量重构为常规桌面页：页头（标题／状态点／周期切换／刷新）＋核心统计行（总 Token、费用、请求数、活跃工具；覆盖缺口按需展开）＋概览／额度／工具／会话／模型／趋势六分区。移除顶栏小组件壳（悬浮圆角卡、Σ 徽标、底部假 live-rate）。颜色/字体全部继承产品主题 token（`--natives-*`，单源 theme 组合），浅色／深色一致；i18n 中英同源新增 `overview`/`activeTools`。数据端点与轮次明细弹窗、请求取消、部分失败重试逻辑不变。

## 5. 真实 DSH Desktop 端到端（目标 1/2/4 的安装与检查）

本机 DSH 0.2.1-rc.3（CDP 驱动实测，脚本与截图在 `.tmp/dsh-goal/`）：

- 四个入口全部注册，`/natives/{space,files,tokenusage,fund}/url?theme=volt` 全 200；逐个点击后主面板均为全屏 iframe（1000×820 逻辑满幅），**无任何 CSP 违例**、不切换会话。
- 四模块截图确认：空间（真实工作区＋仅工作区树）、文件（完整文件管理器，无跨模块入口）、Token 统计（新桌面页＋真实数据 110.8M/≥$32.20/52,964/2）、投资（完整投资终端，真实数据 295,906.65/目标水位 29.5907%）。
- 主题：DSH 深色 → 全部 Volt 深色跟随；浅色路径已在真实 Chrome 顶层验证（tokenusage 浅色截图 `tokenusage-loaded.png`、space/files 浅色断言）。
- 样式统一（目标第 4 条）：四模块与三宿主页共用 `extension/theme.css` 单源 token（模块经 `theme-compose` 组合），未引入任何页面级颜色覆盖；浅色变量无需再调。

## 6. 门禁（2026-10-29 01:10 前后）

`cargo fmt --check`、`cargo test --workspace`（exit 0）、`cargo test -p native-file-host` 124、`cargo test -p app-runtime`；`dsh:check` 7；`extension:check`、`apps:check`、`modules:ui:build`/`check`；perf：`architecture-check`、`perf:extension`、`perf:extension:test`、`perf:native-host`、`perf:model-host` 全部通过（上一轮因沙箱 `ps` EPERM 的三项本轮通过）；`standards:check`（含于 extension:check）、`git diff --check`。

## 7. 未完成与阻塞

1. **生产签名 payload**：仓库只有开发信任根，`scripts/dsh-payload.mjs` 已实现但只能以生产签名产品源为输入；对外分发保持无 payload 形态（对方需已装 Natives），不冒充。
2. **本会话未重新走 tarball 安装通道**：四包以目录链接装入（重启 DSH 已加载全部新代码并实测）；tarball 经 `plugin_manager` 的安装由上一会话验证，本会话无该工具通道。
3. 真实 UI 双写并发、异常关闭 60 秒回收、干净机器验收：未做（同前序记录）。
4. 30 分钟长测：按 `04-performance.md` 复用规则未重跑；本轮改动不触及内存／连接／进程生命周期路径，短测全绿。

## 8. payload 管线干净 HOME 实测（2026-10-29 01:35，开发签名隔离验证）

生产签名材料确认不存在：`/Library/Application Support/Natives` 缺失；`Natives-Local` 安装候选经 `product-trust-dev.public.pem` 验签**通过**（即开发信任根签署）；编译期 `PRODUCT_TRUST_ROOT_HEX` 与 dev 公钥逐字节相同（`b266cdb8…`），全机无生产密钥与轮换钩子。release Core 在轮换前拒绝一切当前可生成的 payload（`APP_DEV_TRUST_ROOT_IN_PRODUCTION`），故对外分发保持无 payload 形态。

在此前提下，用开发签名 payload 在隔离环境把 payload 分支全链验证到「干净机器等价」：

- 隔离 HOME（`.tmp/dsh-clean/home`，checkout 之外）＋四 tarball 解包安装＋随包 payload（`dsh-payload.mjs` 密封：267 文件 / 70.3 MiB，`payload-manifest.json` 逐文件 sha256）。
- 四插件 `resolveCore` 全部落 `payload` 模式 → `verifyPayload` 通过 → payload 内 Core 拉起（隔离 8802，数据写隔离 HOME）→ 嵌入 URL 完整形态（`embed`/`anc=dsh-app%3A%20dsh%3A…`/`standalone=1`/`theme=volt`/43 字符 `#token`）→ 页面 200。8/8 断言通过（`verify-payload.mjs`）。
- 模块全链：payload 内 `hosts/natives-app-runtime` 拉起（临时端口），Token 统计新桌面页渲染（六分区、Volt、standalone），隔离环境零数据呈真实空态（截图 `payload-tokenusage.png`）。
- 过程中修一处测试前提：`cargo test --workspace` 会在刷新后以 workspace 统一 feature 重建 bin，令 `appRuntime.sha256` 过期——payload 密封前必须重新 `refresh-dev-product-source`（已在脚本输出提示，本次实测踩中并修正）。

结论：payload 分支的工程实现（密封、校验、解析、拉起、模块全链）全部实测通过；剩余仅为「以生产签名产品源为输入重新密封＋真机验收」，属机械步骤。

拍板记录：三选项（①提供生产签名产品源路径→`npm run dsh:pack -- --source` 重密封＋真机验收；②明确接受「对方装一次 Natives 产品即可用」形态并维持阻塞登记；③授权放宽早期分发→先补 ADR-0048 D6 修订）已于 2026-10-29 通过交互选项两轮正式呈报，截至本文最后更新未获答复；用户拍板前，交付维持 ② 形态运行（零放宽、零动作），①/③ 的材料或授权到位即切换。

## 9. 一条命令打包与防呆护栏（2026-10-29 01:40）

新增 `scripts/dsh-pack.mjs`（`npm run dsh:pack`）：一条命令产出四个 tarball；带 `--source <产品源>` 时先经 `dsh-payload.mjs` 密封随包 payload 再打包。防呆护栏：不带 `--source` 且任一插件目录残留 `payload/` 时拒绝打包并提示——机制上杜绝开发信任根 payload 被静默分发。护栏拒绝路径与正常打包路径均实测（`dsh:check`、`standards:check`、`git diff --check` 通过）。拍板项维持呈报：生产签名产品源输入 → `npm run dsh:pack -- --source <产品源>` → 真机验收；或明确接受「对方装一次 Natives 产品即可用」形态。
