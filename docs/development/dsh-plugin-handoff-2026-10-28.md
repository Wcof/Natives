# DSH 插件接入 · 交接上下文（2026-10-28）

> 本文件给下一位执行者。目标是让别人只装插件就能在 DeepSeek Harness（DSH）里
> 使用空间／文件／Token 统计／投资四个模块，并且**内容直接占满主区、像 DSH 自己
> 的页面**，不是右侧 Browser 标签，也不是二级菜单。

## 1. 已确认的产品形态

- 四个**独立插件**：`@natives/dsh-space`、`@natives/dsh-files`、
  `@natives/dsh-tokenusage`、`@natives/dsh-fund`。
- 每个插件只有 1 个 `sidebar.panellist` 入口 + 1 个同名 `main` key。
- 点击入口 → 主面板全屏 iframe 加载 Natives 页面；**不切换会话、不开右侧标签**。
- 四个插件可单独安装／启用／卸载；共用同一份 Core、同一数据、同一 8770。
- 现有 Chrome 扩展／本机 Web／Launcher 三种入口保持原样，双形态共存。

决策文档：[ADR-0048](../adr/0048-dsh-plugin-third-entry.md)（Accepted；D5.2 为
2026-10-28 新增的“签名嵌入例外”）。规范：[技术 01](../standards/technical/01-layering.md)
R-T1／R-T8。

## 2. 当前实现地图

```text
integrations/dsh/
├─ runtime/index.js          # 唯一运行时源：解析 Core、护栏、mint URL、Host 路由
├─ runtime/index.test.mjs    # 6 个 node:test
├─ {space,files,tokenusage,fund}/
│  ├─ index.js               # Host：import './runtime.js'，注册 /natives/<m>/url
│  ├─ runtime.js             # 由脚本生成的逐字节副本（勿手改）
│  ├─ client.js              # 1 入口 + 1 主面板（全屏 iframe）
│  ├─ cordis.patch.yml       # insert 1 行
│  ├─ package.json           # dsh.bundle.patch + dsh.client + files
│  ├─ locale/{zh,en}.json
│  └─ icon.svg
scripts/dsh-sync-runtime.mjs # 源 → 四副本；--check 校验漂移
```

Core 侧改动：

- `crates/native-file-host/src/service/embed.rs`（新增）：HMAC-SHA256 签名 grant、
  ancestors 校验、`frame_policy`；TTL 120 秒；3 个单测。
- `service/launcher.rs`：拆 `bootstrap()`；新增 `prepare_url_embedded(target, ancestors)`。
- `main.rs`：`--launcher-url <target> [--embed-ancestors <值>]`。
- `service/server.rs`：`handle_static` 接收完整 path；对带有效 grant 的 HTML 响应输出
  `Content-Security-Policy: frame-ancestors …` 并**省略** `X-Frame-Options`；其他响应
  维持 `SAMEORIGIN`。
- `service/static_files.rs`：`platform/theme-param.js` 加入 Web 内嵌资源。
- `extension/platform/theme-param.js`（新增）＋ 四个页面接入 `?theme=archive|volt`
  （只覆盖本次渲染，不写回 `natives-theme`）。`app.js` 已修：模块 iframe 的
  appearance 使用解析后的主题。

## 3. 关键机制

### 3.1 Core 解析顺序（`runtime/index.js`）

1. `NATIVES_CORE_BIN`（测试/高级）；
2. 源码 checkout 的 `target/debug|release/native-file-host`（仅当插件位于 checkout
   内），并注入同目录 `model-host` 为 `NATIVES_MODEL_HOST_BIN`；
3. 已安装产品源（`NATIVES_DEV_PRODUCT_SOURCE` → `~/.natives-local/product-source`
   → `/Library/Application Support/Natives`）；
4. 随包 `<plugin>/payload`（当前没有，等生产签名）。

**不要**默认设 `NATIVES_STATE_DIR`：debug Core 用 `~/.natives-local`，release Core 用
`~/.natives`；强塞会读写错数据。只有显式 override 才透传。

### 3.2 URL 与签名嵌入

`GET /natives/<module>/url?theme=`（Host 同源只读）→ 调
`native-file-host --launcher-url <target> --embed-ancestors "dsh: <webServer.getOrigin()>"`
→ 返回 `http://127.0.0.1:<port>/<page>?embed=<grant>&anc=<anc>#token=<43>`。

- grant = HMAC(relay secret, `target\nancestors\nexp`)，120 秒；
- ancestors 只允许 `dsh:` 与 `http://127.0.0.1:<port>` / `https://127.0.0.1:<port>`；
- 只有携带有效 grant 的 HTML 响应才 `frame-ancestors`，其余 `SAMEORIGIN`；
- 客户端把该 URL 直接放进 `<iframe>`，全屏 `width/height:100%`。

### 3.3 主题

DSH dark → `?theme=volt`，light → `?theme=archive`。页面读参数覆盖 `data-theme`，
不写回 `natives-theme`。已在真实 Chrome 验证 space/fund/tokenusage 三种页面。

## 4. 当前运行状态（本机）

- DSH Desktop `0.2.1-rc.3`，profile `desktop`，四个包安装为指向仓库目录的 symlink：
  `~/.dsh/profiles/desktop/node_modules/@natives/dsh-*`。
- **空间**已在最后一次改动后重新 remove+install（iframe 客户端 + 新 runtime）。
- **files/tokenusage/fund 尚未重新安装**：磁盘上的 `client.js`/`runtime.js` 已是 iframe
  版，但 Host 进程里加载的仍是旧模块（右栏版）。下一位需要 remove+install 这三个，
  或重启 DSH。
- 四个入口与四个 `main` key 均已注册（`cordis_inspect_query Slots` 验证过）。

## 5. 已验证 / 未验证

已验证：

- Core：`cargo test -p native-file-host` 121 passed；`cargo fmt --check`。
- JS：`npm run dsh:check` 6 passed（含副本一致性、payload/checkout 解析）。
- 安装：`npm pack` 四个自包含 tarball（8 文件，含 runtime.js），`plugin_manager`
  从 tarball 安装成功；移除一个不影响其余；随后切回目录链接便于迭代。
- 嵌入：curl 验证带 grant 无 XFO 且有 `frame-ancestors`；无 grant 有 XFO。
- 真实 Chrome：父页面 `127.0.0.1:8796` iframe 加载签名 URL，渲染“空间”真实页面，
  无 console error。
- 主题：space/fund/tokenusage 的 `data-theme` 与 iframe src 正确，偏好未写回。
- 多 Surface：空间+投资同时开在一个 Core，均渲染。

未验证 / 阻塞：

1. **Desktop 的 `dsh://app` 父来源是否被 `frame-ancestors dsh:` 接受**。这是当前
   唯一的产品形态未知数；用户正要点“空间”确认。若不接受：页面空白/被拒。
2. **生产签名 payload**：仓库只有开发信任根，release 构建恒定返回
   `APP_DEV_TRUST_ROOT_IN_PRODUCTION`，本机也没有 `/Library/Application Support/Natives`。
   因此 T2/T6（payload 打包、干净机器“装了就能用”）不能完成，也不许用开发签名或
   散装二进制冒充。
3. Desktop 真实点击闭环、真实 UI 双写并发。
4. 异常关闭（无 pagehide）Core 回收约 2～3 分钟，未证明 60 秒。

## 6. 下一步（按顺序）

1. **等用户点“空间”**：
   - 主区铺满 → 继续第 2 步；
   - 空白/被拒 → 说明 `dsh:` 不被接受。备选：
     a. 只对有效 grant 使用 `frame-ancestors *`（弱化，需在 ADR 记录残余风险；
        grant 仍短时且 URL 不公开）；
     b. 检测 Desktop 平台，改用 `file://` 或 `app://` 祖先？先实测 DSH 的
        `document.location.origin`（Client 可读）再定；
     c. 回退到 Browser 标签（D5.1）。
     不要反向代理、不要剥 XFO、不要自建 webview。
2. remove+install `files`、`tokenusage`、`fund` 三个包（目录 target），验证四个主面板
   都是全屏页面，互不切换会话。
3. 更新证据文档（`docs/development/evidence/dsh-plugin-t0-2026-10-28.md`）与
   ROADMAP：写清“主面板 iframe”已实测/未实测。
4. 若用户能提供生产签名产品源：再做 `scripts/dsh-payload.mjs`（尚未实现）＋
   干净机器验收。否则保持阻塞，不要造假。

## 7. 常用命令

```bash
# 运行时代码同步（改 runtime/index.js 后必须）
node scripts/dsh-sync-runtime.mjs          # 写入四副本
node scripts/dsh-sync-runtime.mjs --check  # 漂移校验

# JS / Rust
npm run dsh:check
rtk env -u CARGO_TARGET_DIR cargo test -p native-file-host
rtk env -u CARGO_TARGET_DIR cargo fmt --check

# 手工验证签名嵌入（隔离数据/端口）
NATIVES_STATE_DIR=/tmp/x NATIVES_WEB_PORT=8798 NATIVES_WEB_ROOT=$PWD/extension \
  ./target/debug/native-file-host --launcher-url space \
  --embed-ancestors "dsh: http://127.0.0.1:8796"
curl -sD - -o /dev/null "http://127.0.0.1:8798/space.html?embed=…&anc=…"  # 看 CSP

# 安装/重装（plugin_manager）
remove_bundle target=@natives/dsh-space
install_bundle target=/abs/path/integrations/dsh/space
```

## 8. 坑与纪律

- 客户端模块必须用 `window.__ModuleLoader__.load({ id, factory(require) })`；React 从
  `require('react')` 取，不要打包第二份。
- `main` 的非 `conversation` key **没有 Session 绑定**；不要用 `commands.execute`
  （需要 Agent），也不要依赖 session props。
- 插件自建 `<webview>` 会被主进程 `will-attach-webview` 无条件拒绝。
- 不要反向代理 Natives、不要剥 `X-Frame-Options`、不要把页面并入 DSH 同源。
- 运行时副本由脚本生成，**只改 `runtime/index.js`**，然后 sync。
- 产品清单实际文件名是根目录 `product-manifest.json` / `.sig`。
- 工作区里有其他人未提交的改动（`modules/fund/*`、`docs/contracts/*`、`docs/promo/`、
  home 相关脚本）；不要动、不要回滚。
- 不提交/不推送/不发布；测试用 `NATIVES_STATE_DIR` 隔离，别写真实库。
- 当前 DSH GUI 端口每次可能变化；以运行时 context 的 URL 为准。

## 9. 关键文件索引

- 决策：[ADR-0048](../adr/0048-dsh-plugin-third-entry.md)
- 执行提示词（历史）：[dsh-plugin-execution-prompt.md](dsh-plugin-execution-prompt.md)
- 证据：[dsh-plugin-t0-2026-10-28.md](evidence/dsh-plugin-t0-2026-10-28.md)
- 规范：[01-layering.md](../standards/technical/01-layering.md)、
  [05-port-registry.md](../standards/technical/05-port-registry.md)
- 插件：[integrations/dsh](../../integrations/dsh)
- 运行时：[runtime/index.js](../../integrations/dsh/runtime/index.js)
- Embed：[embed.rs](../../crates/native-file-host/src/service/embed.rs)
