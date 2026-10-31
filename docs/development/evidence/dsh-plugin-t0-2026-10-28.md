# DSH 插件入口 T0／T1／T3／T4 证据（2026-10-28）

依据 [ADR-0048](../../adr/0048-dsh-plugin-third-entry.md) 与执行提示词
[dsh-plugin-execution-prompt](../dsh-plugin-execution-prompt.md)。本轮完成 T0 闸门、
T1 Core 增量、T3 共享 runtime 与 T4 四个 Client 插件；T2／T5 完整场景／T6 未完成，
原因见文末。

## 1. 环境与前提

- macOS arm64，DSH `0.2.1-rc.3`（`/Applications/DeepSeek Harness.app`），
  Node 22.22.2，`native-file-host` 为 debug 构建。
- 本会话**没有** `plugin_manager` 工具；`dsh plugin` CLI 只把参数转发给 pnpm
  （`dsh plugin --help`：`forwarding the remaining arguments to pnpm in the profile
  directory`），不做 bundle 选择。因此 T0 的安装验证在**隔离 `DSH_HOME`**
  （`.tmp/dsh-probe/home`）里用 profile patch fixture 完成：
  `node_modules/@natives/dsh-probe` 软链到探针目录，`cordis.patch.yml` 写
  `insert: natives-probe`。真实 `install_bundle` 安装链路未执行。
- 隔离实例 `dsh web` 监听 `127.0.0.1:3080`，用 playwright-core + 本机 Chrome
  无头驱动；未登录、未配置模型，因此**无法创建真实会话**。

## 2. T0 逐项结论

| 项 | 结论 | 证据 |
| --- | --- | --- |
| bundle 包格式被接受（`dsh.bundle.patch` + `dsh.client`） | 成立 | `dsh web --dump-config` 输出含 `- id: natives-probe / name: '@natives/dsh-probe'`；页面加载无 console／page error |
| `sidebar.panellist` 与同名 `main` key 配对 | 成立 | 侧栏出现「Natives 探针」；点击后 `main` 面板渲染出标题、说明与按钮（`PANEL_HAS_HINT: true`、`PANEL_HAS_BUTTON: true`） |
| 入口图标／label 由注册方提供 | 成立 | 首轮 label 渲染为原始 key（`panel.title`），改用 `ctx.locale.register(NS, {zh,en})` + `ctx.locale.bind` 后显示「Natives 探针」；locale JSON 只供插件清单展示 |
| 驻留面板壳内调用 `openTab('browser', {params:{url}})` | **不成立** | 面板内点击按钮抛 `Error: sidebarRight: no session surface is mounted` |
| 先 `selectPanel(null)` 恢复会话再 `openTab` | **成立**（且有会话时第 1 次重试即成功） | 真实会话 `probe session` 在场：`MOUNTED` 由 `undefined` 变为 `session-9d73…`，`PROBE_STATUS: {kind:'recovered', attempt 1}`，`[role=tab]` 出现 `127.0.0.1`，`HAS_8770_HTML: true` |
| Web profile 的 `browser` tab 类型 | 需显式开启 | 未开启时抛 `sidebarRight: no tab type is registered as "browser"`；patch `ui-sidebar-browser: disabled: false` 后通过（Desktop 默认启用） |
| Browser 标签内渲染 Natives 页面 | Web 受限、Desktop 未验证 | Core 对 `space.html` 返回 `200` + `X-Frame-Options: SAMEORIGIN`；Web profile 用 iframe 因而被拒，Desktop 用 `<webview>` 不受该头约束但本轮无法验证 |

根因（源码）：`@deepseek-ai/dsh-client-ui-sidebar-right` 的 `require()` 在 `mounted`
为 `undefined` 或该会话 store 未建立时抛上述错误；README 明确 `mounted` 在「全局面板
替代会话」时为 `undefined`，而 `selectPanel(null)` 才代表「会话占据主栏」。因此驻留壳
无法打开标签，**恢复会话后才能打开**。该结论已写入 ADR-0048 D5.1。

第一次回落尝试用 `selectPanel('conversation')` 失败：`'conversation'` 是 `main` 的
key、不是 panellist id，选择它不会回到会话；改为 `selectPanel(null)` 后成立。

按提示词第 6 节停止条件，上一轮在「驻留壳内打开」失败时未进入 T4；本轮把时序改成
「恢复会话 → 打开 → 成功即退场」，该形态已实测通过，可继续 T3／T4。仍未改为自建
`<webview>`／iframe／反向代理。


## 3. T1 Core 增量

改动：

- `crates/native-file-host/src/service/launcher.rs`
  - 拆分 `prepare_url`（`ensure_service` → `native:bootstrap` → `#token=`）与
    `open_in_browser`；`open_web` 行为不变，仅改为 `open_web_with(target,
    prepare_url, open_in_browser)`。
  - 路由白名单新增 `fund → app.html?app=fund`。
  - 抽出 `valid_token`（43 字符 URL-safe base64）。
- `crates/native-file-host/src/main.rs`
  - 新增 `--launcher-url <target>`：输出单行 `{"url":…}`，**不打开浏览器**；
    非法目标／多余参数 `eprintln` + 退出码 1（与 `--launcher-web` 一致）。
  - `--launcher-web` 输出与分支未改。
- `extension/platform/theme-param.js`（新增，纯函数）
  - `themeParam(search)`：仅接受 `archive|volt`，其余返回 `null`。
  - `resolveTheme(stored, search)`：URL 参数优先，否则已保存偏好，未知值回落
    `archive`；不改写存储。
- 四个页面接入（只覆盖本次渲染，不写回 `natives-theme`）：
  `space.js:298`、`files.js:89`、`app.js:20`（含 storage 监听）、`apps.js:17`。
- `crates/native-file-host/src/service/static_files.rs`：`platform/theme-param.js`
  加入内嵌 Web 资源，Web 入口不依赖产品源是否已刷新。
- `extension/theme-param.test.mjs`（新增）并接入 `extension:check`。

验证：

```text
cargo test -p native-file-host                 118 passed; 0 failed
cargo fmt --check                              no diff
node extension/theme-param.test.mjs            ok
NATIVES_STATE_DIR=.tmp/t1-state NATIVES_WEB_PORT=8799 \
  ./target/debug/native-file-host --launcher-url space
  → {"url":"http://127.0.0.1:8799/space.html#token=<43>"}
  --launcher-url fund        → app.html?app=fund#token=…
  --launcher-url tokenusage  → app.html?app=tokenusage#token=…
  --launcher-url apps        → Natives: local Web URL preparation failed; exit 1
  --launcher-url space extra → 同上；exit 1
  重复调用                    → 同一端口 attach，未起第二个实例
```

未验证：`?theme=volt` 在真实浏览器中的渲染与「刷新后偏好不变」（没有做页面渲染
核验）；`space.html`／`files.html`／`apps.js` 的改动要生效于 Web 入口，需刷新产品源
`ChromeExtension`（现有打包流程，不经 Core 二进制内嵌）。

### 3.1 门禁

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过（无差异） |
| `cargo test -p native-file-host` | 118 passed / 0 failed |
| `npm run standards:check` | 通过 |
| `npm run extension:check` | 通过（清空 `dist/dev-extension` 后；首轮被 WorkBuddy 安全删除护栏按 172 文件阈值拦下） |
| `npm run perf:extension` | 通过 |
| `npm run perf:native-host` | **失败**（`spawnSync ps EPERM` → rss unsupported；目录 p95 49.06 ms／50 ms 通过） |
| `npm run perf:model-host` | **失败**（`could not inspect process tree`，同一 `ps` 限制） |
| `perf:extension:test`（`perf:check` 内） | **失败**（`cpu-probe` 调用 `ps` 被拒） |
| `npm run apps:check` | 通过 |
| `git diff --check` | 通过 |

三条性能失败同因：本机沙箱拒绝 `ps`，`scripts/perf/cpu-probe.mjs` 无法枚举进程树。
与本轮改动无关，未修改这些脚本或放宽预算。

## 3.2 T3 共享 runtime（`@natives/dsh-runtime`）

`integrations/dsh/runtime/index.js` 是一个**库**，不是独立行：四个插件各自的 Host
半部 import 它，因此只有一份 runtime 代码、一份解析与护栏逻辑，不产生第二个进程、
端口或数据 writer。

- 解析顺序：`NATIVES_CORE_BIN`（仅测试）→ `NATIVES_DEV_PRODUCT_SOURCE` →
  `~/.natives-local/product-source` → `/Library/Application Support/Natives`，
  逐源查 `hosts/native-file-host`、`native-file-host`（`installers/macos/build-pkg.sh`
  两处都装）。
- 子进程 env：只加 `NATIVES_STATE_DIR` 与 `NATIVES_DEV_PRODUCT_SOURCE`，
  并**删除** `NATIVES_CORE_BIN`，不改 `process.env`。
- 护栏：读 `<apps_root 父目录>/product/activation.json` 的 `modules[*].dataSchemaVersion`，
  与所源清单比较；更高即拒绝，且**不启动** Core。
- URL：复用 T1 的 `--launcher-url`（同一 bootstrap 链），返回前用 `URL` 追加
  `theme=`，token 留在 fragment 末尾。
- 路由：`GET /natives/<module>/url?theme=` → `{url}`，精确匹配、同源校验、
  `Cache-Control: no-store`，不代理页面、不转发业务数据。

实测（隔离 `DSH_HOME` + fixture 产品源 + 隔离 `NATIVES_STATE_DIR`）：

```text
BASELINE       200  .../app.html?app=fund&theme=archive#token=…
SCHEMA_TOO_NEW 409  {code:"schema-too-new",appId:"fund",recorded:999,runtime:29}
AFTER_REMOVE   200  （删除伪造 activation 后恢复）
DARK           fund/url?theme=volt → ...&theme=volt#token=…
```

单元测试 `integrations/dsh/runtime/index.test.mjs`（5 项，`npm run dsh:check`，已接入
`extension:check`）：护栏三态、未知模块不阻塞、Core 解析顺序与缺失、子进程 env
不泄漏 override、目标白名单、launcher 非 JSON 失败、theme 追加且 fragment 不变、
更高 schema **不启动** Core。

## 3.3 T4 四个 Client 插件

`integrations/dsh/{space,files,tokenusage,fund}/`：各自 1 个 `sidebar.panellist`
（`natives-space` 等）与同名 `main` key，1 个主面板壳，无二级菜单；locale 走
`ctx.locale.register` + `bind`（`meta.title`／`description` 供插件清单，运行期文案
内联中英双语）；图标各自原创 SVG，响应 `size`／`active`。

打开时序按 ADR-0048 D5.1：面板挂载 → `fetch` 本插件路由 → `selectPanel(null)`
恢复会话 → `openTab('browser')`（200 ms 间隔最多重试 10 次）。成功即退场；失败驻留壳内
显示具体原因（`core-missing`／`schema-too-new`／`launcher-failed`／`open-failed`／`network`）。

四个入口依次点击的实测结果：

```text
空间       tabs=[对话,轨迹,127.0.0.1]
文件       tabs=[…,127.0.0.1]
Token 统计 tabs=[…,127.0.0.1]
投资       tabs=[…,127.0.0.1]

space/url?theme=archive      → http://127.0.0.1:8799/space.html?theme=archive#token=…
files/url?theme=archive      → …/files.html?theme=archive#token=…
tokenusage/url?theme=archive → …/app.html?app=tokenusage&theme=archive#token=…
fund/url?theme=archive       → …/app.html?app=fund&theme=archive#token=…
```

控制台仅有 4 条 `X-Frame-Options` 拒绝——Web profile 的 Browser 用 iframe，属预期；
Desktop 的 Browser 是 `<webview>`，不受该头约束（未实测）。

独立性（fixture 级，非 `install_bundle`）：从 patch 删掉 `natives-files` 后重启，
侧栏只剩「空间／Token 统计／投资」，`/natives/files/url` 返回 404，其余三个路由仍 200。

单 Core：四次调用都落在同一端口 8799 并逐个 attach；页面关闭后 Core 自行退出，
`lsof -iTCP:8799` 已无监听（60 秒回收窗口内）。


## 4. T2／T6 阻塞（生产签名）

`crates/native-file-host/src/app_signing.rs`：

```rust
pub const PRODUCT_TRUST_ROOT_HEX: &str = "b266…caf2"; // dev trust root
...
if is_production_build() {
    return Err(AppError::InvalidState(
        "APP_DEV_TRUST_ROOT_IN_PRODUCTION: release trust root is required".into(),
    ));
}
```

即 release 构建**恒定**拒绝任何用该开发密钥签名的清单，仓库里也不存在生产公钥。
因此现在无法产出「生产签名的 sealed 产品源」，payload 打包与干净机器验收
（T2／T6）不能开始；这与提示词 3.1 第 1 条一致。散装二进制和开发签名源都不得
用来拼 payload。

## 5. 未验证与下一步

1. **Desktop 端到端**：Desktop 的页面在 `dsh://app` 下运行，主进程把非静态资源请求
   `forwardWebRequest(request, hostUrl, hostCookie)` 转给本机 webserver（已实测
   Desktop 在 `127.0.0.1:59252` 监听并返回 `dsh web authentication required`），
   因此 `ctx.webServer` 通道在 Desktop 存在。但本轮全部运行时验证都在 Web profile
   完成；Desktop 的 `<webview>` 是否正常渲染 Natives 页面、租约是否放行，未实测。
2. 真实 `install_bundle` 安装、四个插件的独立启用／卸载未执行（本会话无该通道）；
   独立性只在 fixture 级验证（删 patch 行）。
3. 隔离测试用了 `NATIVES_CORE_BIN`（首轮）与 fixture 产品源（护栏轮），真实产品源
   目录下的 `hosts/native-file-host` 解析未在已安装环境验证。
4. 主题：路由已按 `colorScheme` 返回 `theme=volt`，但页面内 `data-theme` 的实际渲染
   未核验（Web profile 被 XFO 挡住）；`space.html`／`files.html`／`apps.js` 的
   `?theme=` 改动需刷新产品源 `ChromeExtension` 才在 Web 入口生效。
5. T5 完整生命周期（Chrome 与本机 Web 同时打开、双写、重启 attach）未执行；只观察到
   隔离实例在页面关闭后自行退出、无残留监听。
6. 分发（T6）仍被生产信任根阻塞，见第 4 节。

## 6. 续作（2026-10-28 22:19，真实安装与运行时修正）

上一轮留下的两个实现缺陷已修复，并补做了可自动化的验证。

### 6.1 runtime 分发方式

安装器对本地目录只做 `link`，不安装包的 `dependencies`；原先四个插件依赖
`@natives/dsh-runtime`，在另一台机器上会解析失败（本机是靠手工
`integrations/dsh/node_modules` 软链才通过）。现改为：

- `integrations/dsh/runtime/index.js` 仍是唯一源；
- `scripts/dsh-sync-runtime.mjs` 生成四份逐字节相同的 `runtime.js`，
  `--check` 校验漂移；
- 四个包删除 `@natives/dsh-runtime` 依赖，`files` 增加 `runtime.js`；
- 删除手工软链后，四个包各自 `import('./index.js')` 均通过；
- `npm run dsh:check` 现在先校验副本一致，再跑 runtime 单测。

### 6.2 state dir 与清单名

- 不再给子进程强塞 `NATIVES_STATE_DIR`：debug Core 用 `~/.natives-local`、
  release Core 用 `~/.natives`，强塞会导致另一个布局的数据被读写。
  只有显式 override 才透传；护栏同时查看两处 `product/activation.json`。
- 实际产品清单名是根目录 `product-manifest.json` / `.sig`
  （`app_store/product.rs:19`），ADR 与提示词已更正。

### 6.3 真实安装（替换 fixture）

`npm pack` 生成四个 tarball（各 8 个文件，含 `runtime.js`，无 `node_modules`）。
`plugin_manager install_bundle` 逐个安装：

- `space` 从 tarball 安装后，`~/.dsh/profiles/desktop/node_modules/@natives/dsh-space`
  是实体目录，`runtime.js` 在其中，证明包自包含；
- 四个装齐后，`sidebar.panellist` 占用者为 `natives-space`、`natives-files`、
  `natives-tokenusage`、`natives-fund`，`main` 同名 key 全部 active；
- 移除 `@natives/dsh-files` 后该入口消失、其余三个保留；重装后恢复；
- 为继续迭代，最终把四个包重装为指向仓库目录的链接（代码随仓库更新）。

### 6.4 主题与页面（真实 Chrome）

隔离 Core：`NATIVES_STATE_DIR=/tmp/natives-dsh-theme-state`、
`NATIVES_WEB_PORT=8798`、`NATIVES_WEB_ROOT=<repo>/extension`，用
`--launcher-url` 取 URL，再用 playwright-core 驱动本机 Chrome 顶层加载：

```text
space.html?theme=volt      → data-theme=volt,   natives-theme=null
space.html?theme=archive   → data-theme=archive,natives-theme=null
app.html?app=fund&theme=volt        → data-theme=volt,   title=投资 · Natives
app.html?app=tokenusage&theme=archive → data-theme=archive,title=Token 统计 · Natives
```

说明 `?theme=` 生效且未写回偏好；`fund` / `tokenusage` 页面可完成激活与渲染。
另修复：owner 页把解析后的 appearance 也传给模块 iframe（原实现用未覆盖的
`theme` 变量，`?theme=volt` 时 iframe 仍可能按浅色渲染）；复测
`app.html?app=fund&theme=volt` 的 iframe src 为 `http://127.0.0.1:59292/?theme=volt`。

### 6.5 多 Surface 与回收

同一 Chrome 两个 context 同时打开空间与投资，均渲染真实页面，Core 只有一个
（8798）。关闭浏览器后 Core 未在 60 秒内退出，约 2～3 分钟后才释放端口；
异常关闭（无 pagehide）下的回收时间不符合“60 秒”预期，正常关标签未单独验证。

### 6.6 仍未完成

- **T2/T6**：仓库只有开发信任根，release 构建会拒绝开发签名
  （`APP_DEV_TRUST_ROOT_IN_PRODUCTION`），也没有生产签名的产品源；payload
  打包与“干净机器”验收无法进行，未用散装二进制或开发签名冒充。
- **Desktop 点击闭环**：Desktop 无 CDP，无法自动点击四个入口；入口注册与
  `selectPanel(null) → openTab` 时序已在 Web profile 与源码层验证，Desktop
  的真实标签打开需要人工点一次确认。
- **T5 双写**：未在真实 UI 中做“两边同时写同一空间”的并发验证。

### 6.7 最终门禁（2026-10-28 22:2x）

| 命令 | 结果 |
| --- | --- |
| `npm run dsh:check` | 5 passed（含副本一致性检查） |
| `cargo fmt --check` | 通过 |
| `cargo test -p native-file-host` | 118 passed / 0 failed |
| `npm run extension:check` | 通过（含 standards、dsh、theme-param） |
| `npm run perf:check` | 首次 `perf:native-host` 目录 p95 51.46 ms＞50 ms 失败；复跑 45.62 ms，整体 `perf:check` 41.25 ms 通过；两次结果均保留 |
| `npm run standards:check`、`git diff --check` | 通过 |

安装状态：当前 desktop profile 的四个包最终为指向仓库目录的链接（便于迭代）；
tarball 自包含安装、单插件移除与重装已分别验证。

## 7. 点击报 `core-missing` 的根因与修复（2026-10-28 22:56）

用户点击入口后报“未找到 Natives 产品源”。根因不是需要手动起服务，而是解析链只看
产品源目录：开发产品源（`~/.natives-local/product-source`）的 `hosts/` 只有
`natives-app-runtime`，**没有 `native-file-host`**；Core 二进制只在
`target/debug`（或 release）里。

修复后的解析顺序（`runtime/index.js`）：

1. `NATIVES_CORE_BIN`（测试/高级）；
2. 源码 checkout 的 `target/debug|release/native-file-host`（仅当插件位于 checkout
   内），并注入同目录 `model-host` 为 `NATIVES_MODEL_HOST_BIN`；
3. 已安装产品源（`NATIVES_DEV_PRODUCT_SOURCE` → `~/.natives-local/product-source`
   → `/Library/Application Support/Natives`）；
4. 随包 `<plugin>/payload`（给没有 Natives 的机器）。

验证：

- 从安装路径
  `~/.dsh/profiles/desktop/node_modules/@natives/dsh-space/runtime.js` 解析得到
  `{mode:'dev', binary:target/debug/native-file-host, modelHost:target/debug/model-host}`；
- 隔离状态与端口下 `openModuleUrl({target:'fund', theme:'volt'})` 自动启动 Core 并
  返回可渲染 URL（`投资 · Natives`）；
- 单测覆盖 payload 优先于空源、checkout 识别、无 checkout 且无 payload 时拒绝。

因此不需要手动起后台服务：点击入口时才启动 Core，关闭页面后由既有回收逻辑释放。
“装了就能用”在开发机上已满足；别人机器上仍依赖第 4 条 payload，而 payload 的
生产签名仍受第 4 节阻塞。


